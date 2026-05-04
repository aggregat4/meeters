use std::collections::HashSet;
use std::thread;

use chrono::prelude::*;
use chrono_tz::Tz;
use gtk::prelude::*;
use ureq::Agent;

use crate::config::Config;
use crate::domain::{Event, RefreshState};
use crate::CalendarMessages::{DayEvents, EventNotification, RefreshStateChanged};
use domain::CalendarError;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod binary_search;
mod config;
mod custom_timezone;
mod domain;
mod gui;
mod ical_util;
mod logging;
mod meeters_ical;
mod timezones;
mod windows_timezones;

use std::sync::Arc;

fn get_ical(url: &str) -> Result<String, CalendarError> {
    log::debug!("fetching calendar data");
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

fn get_events_for_interval(
    events: Vec<Event>,
    start_time: DateTime<Tz>,
    end_time: DateTime<Tz>,
) -> Vec<Event> {
    let mut filtered_events = events
        .into_iter()
        .filter(|e| {
            if e.all_day && e.start_timestamp == e.end_timestamp {
                e.start_timestamp >= start_time && e.start_timestamp <= end_time
            } else {
                e.start_timestamp < end_time && e.end_timestamp > start_time
            }
        })
        .collect::<Vec<_>>();
    filtered_events.sort_by(|a, b| Ord::cmp(&a.start_timestamp, &b.start_timestamp));
    filtered_events
}

/// Number of refresh attempts kept in the in-memory tray log
const REFRESH_LOG_CAPACITY: usize = 100;

enum CalendarMessages {
    DayEvents(Vec<Vec<Event>>), // Vector of events for each day
    EventNotification(Event),
    RefreshStateChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EventNotificationKey {
    summary: String,
    meeturl: Option<String>,
    start_timestamp: DateTime<Tz>,
    end_timestamp: DateTime<Tz>,
}

fn event_notification_key(event: &Event) -> EventNotificationKey {
    EventNotificationKey {
        summary: event.summary.clone(),
        meeturl: event.meeturl.clone(),
        start_timestamp: event.start_timestamp,
        end_timestamp: event.end_timestamp,
    }
}

fn send_calendar_message(
    sender: &async_channel::Sender<CalendarMessages>,
    message: CalendarMessages,
) -> bool {
    if let Err(e) = sender.send_blocking(message) {
        log::error!("could not dispatch calendar message to GUI thread: {}", e);
        false
    } else {
        true
    }
}

fn events_to_notify<'a>(
    events: &'a [Event],
    now: DateTime<Local>,
    warning_time_seconds: i64,
    notified_event_keys: &HashSet<EventNotificationKey>,
) -> Vec<&'a Event> {
    events
        .iter()
        .filter(|event| {
            let time_distance_from_now = event.start_timestamp.signed_duration_since(now);
            time_distance_from_now.num_seconds() > 0
                && time_distance_from_now.num_seconds() <= warning_time_seconds
                && !notified_event_keys.contains(&event_notification_key(event))
        })
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging::init_from_env();

    let config = Config::load()?;
    log::info!("local timezone configured as {}", config.local_tz_iana);

    let local_tz = config.local_tz;
    let ical_url = config.ical_url.clone();
    let show_event_notification = config.show_event_notification;
    let use_zoommtg = config.use_zoommtg;
    let polling_interval_ms = config.polling_interval_ms;
    let event_warning_time_seconds = config.event_warning_time_seconds;
    let future_days = config.future_days;

    let refresh_state = Arc::new(std::sync::Mutex::new(RefreshState::new(
        REFRESH_LOG_CAPACITY,
    )));

    // Initialize GUI components
    let (mut indicator, window_manager, dbus_receiver) = gui::initialize_gui(
        config.start_hour,
        config.end_hour,
        config.future_days,
        Arc::clone(&refresh_state),
    );

    // Create a message passing channel so we can communicate safely with the main GUI thread from our worker thread
    let (events_sender, events_receiver) = async_channel::bounded::<CalendarMessages>(10);
    let window_manager_clone = Arc::clone(&window_manager);

    glib::MainContext::default().spawn_local(async move {
        while let Ok(event_message) = events_receiver.recv().await {
            match event_message {
                DayEvents(day_events) => {
                    // Update window manager with new events
                    {
                        let mut wm = window_manager_clone.lock().unwrap();
                        wm.update_events(day_events.clone());
                    }

                    // Only show today's events in the indicator menu
                    let empty_events = Vec::new();
                    let today_events = day_events.first().unwrap_or(&empty_events);
                    gui::create_indicator_menu(
                        today_events,
                        &mut indicator,
                        Arc::clone(&window_manager_clone),
                    );
                }
                EventNotification(event) => {
                    if show_event_notification {
                        gui::show_event_notification(event);
                    }
                }
                RefreshStateChanged => {
                    let today_events = {
                        let wm = window_manager_clone.lock().unwrap();
                        wm.today_events()
                    };
                    gui::create_indicator_menu(
                        &today_events,
                        &mut indicator,
                        Arc::clone(&window_manager_clone),
                    );
                }
            }
        }
        log::warn!("calendar GUI message channel closed");
    });

    // Handle D-Bus requests in the main GTK thread
    let window_manager_clone = Arc::clone(&window_manager);

    glib::MainContext::default().spawn_local(async move {
        while let Ok((action, _)) = dbus_receiver.recv().await {
            let mut wm = window_manager_clone.lock().unwrap();
            match action.as_str() {
                "show" => wm.show_window(),
                "close" => {
                    if let Some(window) = &wm.current_window {
                        window.hide();
                    }
                }
                "toggle" => wm.toggle_window(),
                _ => (),
            }
        }
        log::warn!("D-Bus GUI message channel closed");
    });

    // start the background thread for calendar work
    thread::spawn(move || {
        let mut last_download_time = 0;
        let mut last_events: Vec<Event> = vec![];
        let mut notified_event_keys: HashSet<EventNotificationKey> = HashSet::new();
        loop {
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time must flow")
                .as_millis();
            if last_download_time == 0 || current_time - last_download_time > polling_interval_ms {
                last_download_time = current_time;
                match get_ical(&ical_url)
                    .and_then(|t| meeters_ical::extract_events(&t, &local_tz, use_zoommtg))
                {
                    Ok(events) => {
                        {
                            let mut state = refresh_state.lock().unwrap();
                            state.record_success(events.len());
                        }
                        log::info!("successfully got {} events", events.len());
                        let local_date = Local::now().date_naive();

                        // Get events for each day
                        let mut day_events = Vec::new();

                        // Process each day
                        for day_offset in 0..=future_days {
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
                            log::debug!(
                                "There are {} events for day {}: {:?}",
                                day_events_list.len(),
                                day_offset,
                                day_events_list
                            );
                            day_events.push(day_events_list);
                        }

                        // Store today's events for notifications
                        last_events = day_events[0].clone();
                        let todays_event_keys = last_events
                            .iter()
                            .map(event_notification_key)
                            .collect::<HashSet<_>>();
                        notified_event_keys.retain(|key| todays_event_keys.contains(key));

                        if !send_calendar_message(&events_sender, DayEvents(day_events)) {
                            break;
                        }
                    }
                    Err(e) => {
                        {
                            let mut state = refresh_state.lock().unwrap();
                            state.record_failure(e.msg.clone());
                        }
                        if !send_calendar_message(&events_sender, RefreshStateChanged) {
                            break;
                        }
                        log::error!("error getting events: {}", e.msg);
                    }
                }
            }
            // Notify once for each event that starts within the configured warning window.
            let now = Local::now();
            let immediate_upcoming_events = events_to_notify(
                &last_events,
                now,
                event_warning_time_seconds,
                &notified_event_keys,
            );
            for immediate_upcoming_event in immediate_upcoming_events {
                if !send_calendar_message(
                    &events_sender,
                    EventNotification(immediate_upcoming_event.clone()),
                ) {
                    return;
                }
                notified_event_keys.insert(event_notification_key(immediate_upcoming_event));
            }
            thread::sleep(std::time::Duration::from_secs(5));
        }
    });

    // Run the GUI main loop
    gui::run_gui_main_loop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn berlin_datetime(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Tz> {
        chrono_tz::Europe::Berlin
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
    }

    fn event(summary: &str, start_timestamp: DateTime<Tz>, end_timestamp: DateTime<Tz>) -> Event {
        Event {
            summary: summary.to_string(),
            description: String::new(),
            location: String::new(),
            meeturl: None,
            all_day: false,
            start_timestamp,
            end_timestamp,
        }
    }

    fn all_day_event(
        summary: &str,
        start_timestamp: DateTime<Tz>,
        end_timestamp: DateTime<Tz>,
    ) -> Event {
        Event {
            all_day: true,
            ..event(summary, start_timestamp, end_timestamp)
        }
    }

    fn filter_summaries(events: Vec<Event>) -> Vec<String> {
        let start = berlin_datetime(2026, 4, 27, 0, 0);
        let end = berlin_datetime(2026, 4, 27, 23, 59);

        get_events_for_interval(events, start, end)
            .into_iter()
            .map(|event| event.summary)
            .collect()
    }

    #[test]
    fn interval_filter_includes_events_starting_at_interval_start() {
        let summaries = filter_summaries(vec![event(
            "starts at boundary",
            berlin_datetime(2026, 4, 27, 0, 0),
            berlin_datetime(2026, 4, 27, 1, 0),
        )]);

        assert_eq!(summaries, vec!["starts at boundary"]);
    }

    #[test]
    fn interval_filter_excludes_events_ending_at_interval_start() {
        let summaries = filter_summaries(vec![event(
            "ends at boundary",
            berlin_datetime(2026, 4, 26, 23, 0),
            berlin_datetime(2026, 4, 27, 0, 0),
        )]);

        assert!(summaries.is_empty());
    }

    #[test]
    fn interval_filter_excludes_events_starting_at_interval_end() {
        let summaries = filter_summaries(vec![event(
            "starts after day",
            berlin_datetime(2026, 4, 27, 23, 59),
            berlin_datetime(2026, 4, 28, 1, 0),
        )]);

        assert!(summaries.is_empty());
    }

    #[test]
    fn interval_filter_includes_events_crossing_midnight_into_interval() {
        let summaries = filter_summaries(vec![event(
            "crosses midnight",
            berlin_datetime(2026, 4, 26, 23, 30),
            berlin_datetime(2026, 4, 27, 0, 30),
        )]);

        assert_eq!(summaries, vec!["crosses midnight"]);
    }

    #[test]
    fn interval_filter_includes_events_crossing_midnight_out_of_interval() {
        let summaries = filter_summaries(vec![event(
            "crosses out",
            berlin_datetime(2026, 4, 27, 23, 30),
            berlin_datetime(2026, 4, 28, 0, 30),
        )]);

        assert_eq!(summaries, vec!["crosses out"]);
    }

    #[test]
    fn interval_filter_includes_single_day_zero_duration_all_day_events() {
        let summaries = filter_summaries(vec![all_day_event(
            "all day",
            berlin_datetime(2026, 4, 27, 0, 0),
            berlin_datetime(2026, 4, 27, 0, 0),
        )]);

        assert_eq!(summaries, vec!["all day"]);
    }

    #[test]
    fn interval_filter_sorts_events_by_start_time() {
        let summaries = filter_summaries(vec![
            event(
                "later",
                berlin_datetime(2026, 4, 27, 11, 0),
                berlin_datetime(2026, 4, 27, 12, 0),
            ),
            event(
                "earlier",
                berlin_datetime(2026, 4, 27, 9, 0),
                berlin_datetime(2026, 4, 27, 10, 0),
            ),
        ]);

        assert_eq!(summaries, vec!["earlier", "later"]);
    }

    #[test]
    fn notification_candidate_selects_next_event_inside_warning_window() {
        let now = berlin_datetime(2026, 4, 27, 9, 0).with_timezone(&Local);
        let candidate = event(
            "candidate",
            berlin_datetime(2026, 4, 27, 9, 0) + chrono::Duration::seconds(30),
            berlin_datetime(2026, 4, 27, 9, 30),
        );
        let too_late = event(
            "too late",
            berlin_datetime(2026, 4, 27, 9, 2),
            berlin_datetime(2026, 4, 27, 9, 30),
        );

        let events = vec![candidate, too_late];
        let notified_event_keys = HashSet::new();
        let selected = events_to_notify(&events, now, 60, &notified_event_keys);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].summary, "candidate");
    }

    #[test]
    fn notification_candidates_include_same_start_time_events() {
        let now = berlin_datetime(2026, 4, 27, 9, 0).with_timezone(&Local);
        let first = event(
            "first",
            berlin_datetime(2026, 4, 27, 9, 0) + chrono::Duration::seconds(30),
            berlin_datetime(2026, 4, 27, 9, 30),
        );
        let second = event(
            "second",
            berlin_datetime(2026, 4, 27, 9, 0) + chrono::Duration::seconds(30),
            berlin_datetime(2026, 4, 27, 10, 0),
        );
        let events = vec![first, second];
        let notified_event_keys = HashSet::new();

        let selected = events_to_notify(&events, now, 60, &notified_event_keys);
        let summaries = selected
            .into_iter()
            .map(|event| event.summary.as_str())
            .collect::<Vec<_>>();

        assert_eq!(summaries, vec!["first", "second"]);
    }

    #[test]
    fn notification_candidates_skip_already_notified_event() {
        let now = berlin_datetime(2026, 4, 27, 9, 0).with_timezone(&Local);
        let already_notified = event(
            "already notified",
            berlin_datetime(2026, 4, 27, 9, 0) + chrono::Duration::seconds(30),
            berlin_datetime(2026, 4, 27, 9, 30),
        );
        let notified_event_keys = HashSet::from([event_notification_key(&already_notified)]);
        let events = vec![already_notified];

        let selected = events_to_notify(&events, now, 60, &notified_event_keys);

        assert!(selected.is_empty());
    }
}
