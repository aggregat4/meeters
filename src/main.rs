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

/// Number of refresh attempts kept in the in-memory tray log
const REFRESH_LOG_CAPACITY: usize = 100;

enum CalendarMessages {
    DayEvents(Vec<Vec<Event>>), // Vector of events for each day
    EventNotification(Event),
    RefreshStateChanged,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    println!("Local Timezone configured as {}", config.local_tz_iana);

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

    // Set up a periodic check for event messages
    let events_receiver_clone = events_receiver.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        while let Ok(event_message) = events_receiver_clone.try_recv() {
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
        glib::ControlFlow::Continue
    });

    // Handle D-Bus requests in the main GTK thread
    let window_manager_clone = Arc::clone(&window_manager);
    let dbus_receiver_clone = dbus_receiver.clone();

    // Set up a periodic check for D-Bus messages
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        while let Ok((action, _)) = dbus_receiver_clone.try_recv() {
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
                        println!("Successfully got {:?} events", events.len());
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

                        events_sender.send_blocking(DayEvents(day_events)).unwrap();
                    }
                    Err(e) => {
                        {
                            let mut state = refresh_state.lock().unwrap();
                            state.record_failure(e.msg.clone());
                        }
                        events_sender.send_blocking(RefreshStateChanged).unwrap();
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
                    && time_distance_from_now.num_seconds() <= event_warning_time_seconds
            });
            if let Some(next_immediate_upcoming_event) = potential_next_immediate_upcoming_event {
                if last_notification_start_time.is_none()
                    || next_immediate_upcoming_event.start_timestamp
                        != last_notification_start_time.unwrap()
                {
                    events_sender
                        .send_blocking(EventNotification(next_immediate_upcoming_event.clone()))
                        .unwrap();
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
