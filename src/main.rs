use std::path::Path;
use std::path::PathBuf;
use std::thread;

use chrono::prelude::*;
use chrono_tz::Tz;
use directories::ProjectDirs;
use gtk::prelude::*;
use gtk::Menu;
use libappindicator::{AppIndicator, AppIndicatorStatus};
use notify_rust::Notification;
// use gtk::DrawingArea;  // Unused import
// use gtk::cairo;        // Unused import

use crate::domain::Event;
use crate::CalendarMessages::{EventNotification, TodayEvents};
use domain::CalendarError;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod binary_search;
mod custom_timezone;
mod domain;
mod ical_util;
mod meeters_ical;
mod timezones;
mod windows_timezones;

fn get_ical(url: &str) -> Result<String, CalendarError> {
    println!("trying to fetch ical");
    match ureq::get(url).timeout(Duration::new(10, 0)).call() {
        Ok(response) => match response.into_string() {
            Ok(body) => Ok(body),
            Err(e) => Err(CalendarError {
                msg: format!("Error getting calendar response body as text: {}", e),
            }),
        },
        Err(e) => Err(CalendarError {
            msg: format!("Error getting ical from url: {}", e),
        }),
    }
}

fn has_icons(dir: &Path) -> bool {
    let normal_icon_path = dir.with_file_name("meeters-appindicator.png");
    let error_icon_path = dir.with_file_name("meeters-appindicator-error.png");
    normal_icon_path.exists() && error_icon_path.exists()
}

fn find_icon_path() -> Option<PathBuf> {
    if let Ok(exe_path) = std::env::current_exe() {
        if has_icons(&exe_path) {
            return Some(exe_path);
        }
    }
    let config_dir = get_config_directory();
    if has_icons(&config_dir) {
        return Some(config_dir);
    }
    None
}

fn set_error_icon(indicator: &mut AppIndicator) {
    if let Some(icon_path) = find_icon_path() {
        indicator.set_icon(
            icon_path
                .with_file_name("meeters-appindicator-error.png")
                .to_str()
                .unwrap(),
        );
    }
}

fn set_some_meetings_left_icon(indicator: &mut libappindicator::AppIndicator) {
    if let Some(icon_path) = find_icon_path() {
        indicator.set_icon(
            get_icon_path_with_fallbak(
                icon_path,
                "meeters-appindicator-somemeetingsleft.png".to_string(),
            )
            .to_str()
            .unwrap(),
        );
    }
}

fn set_no_meetings_left_icon(indicator: &mut libappindicator::AppIndicator) {
    if let Some(icon_path) = find_icon_path() {
        indicator.set_icon(
            get_icon_path_with_fallbak(
                icon_path,
                "meeters-appindicator-nomeetingsleft.png".to_string(),
            )
            .to_str()
            .unwrap(),
        );
    }
}

fn get_icon_path_with_fallbak(icon_path: PathBuf, icon_filename: String) -> PathBuf {
    let nomeetingsleft_icon_path = icon_path.with_file_name(icon_filename);
    if !nomeetingsleft_icon_path.exists() {
        return icon_path.with_file_name("meeters-appindicator.png");
    } else {
        return nomeetingsleft_icon_path;
    }
}

fn create_indicator() -> AppIndicator {
    let mut indicator = AppIndicator::new("meeters", "");
    indicator.set_status(AppIndicatorStatus::Active);
    match find_icon_path() {
        Some(icon_path) => {
            // println!("ICON THEME PATH FOUND {}", icon_path.to_str().unwrap());
            // including resources into a package is unsolved, except perhaps for something like https://doc.rust-lang.org/std/macro.include_bytes.html
            // for our purposes this should probably be a resource in the configuration somewhere
            indicator.set_icon(
                icon_path
                    .with_file_name("meeters-appindicator.png")
                    .to_str()
                    .unwrap(),
            );
            indicator
        }
        None => {
            indicator.set_icon_full("x-office-calendar", "icon");
            indicator
        } /*  */
    }
}

fn open_meeting(meet_url: &str) {
    match gtk::show_uri(None, meet_url, gtk::current_event_time()) {
        Ok(_) => (),
        Err(e) => eprintln!("Error trying to open the meeting URL: {}", e),
    }
}

struct TimelineView {
    container: gtk::Box,
    events: Vec<Event>,
}

impl TimelineView {
    fn new(events: Vec<Event>) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.set_margin_start(12);
        container.set_margin_end(12);
        container.set_margin_top(12);
        container.set_margin_bottom(12);

        // Constants for layout
        let start_hour: i32 = 7;  // 7 AM
        let end_hour: i32 = 20;   // 8 PM
        let hour_height: i32 = 60;  // Height for one hour
        let time_label_width: i32 = 50;
        let spacing: i32 = 10;

        // Create the main layout container
        let layout_box = gtk::Box::new(gtk::Orientation::Horizontal, spacing);
        layout_box.set_hexpand(true);

        // Time labels column and meeting area (both using Fixed for exact positioning)
        let time_column = gtk::Fixed::new();
        time_column.set_size_request(time_label_width, -1);
        
        let meeting_area = gtk::Fixed::new();
        meeting_area.set_hexpand(true);

        // Add hour markers and grid lines
        for hour in start_hour..=end_hour {
            let y_position = (hour - start_hour) * hour_height;
            
            // Hour label
            let label = gtk::Label::new(Some(&format!("{:02}:00", hour)));
            label.set_xalign(1.0);
            label.set_margin_end(5);
            time_column.put(&label, 0, y_position);

            // Hour separator with styling
            let separator = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            separator.set_hexpand(true);
            let style_context = separator.style_context();
            
            // Different styles for start/end of day vs regular hours
            let css = if hour == start_hour || hour == end_hour {
                "box { background-color: rgba(100, 100, 100, 0.3); min-height: 2px; }"
            } else {
                "box { background-color: rgba(200, 200, 200, 0.15); min-height: 1px; }"
            };
            
            let provider = gtk::CssProvider::new();
            provider.load_from_data(css.as_bytes()).unwrap();
            style_context.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            
            meeting_area.put(&separator, 0, y_position);
        }

        // Current time indicator
        let now = Local::now();
        let current_hour = now.hour() as i32;
        let current_minute = now.minute() as i32;
        if current_hour >= start_hour && current_hour <= end_hour {
            let minutes_from_start = (current_hour - start_hour) * 60 + current_minute;
            let y_position = (minutes_from_start * hour_height) / 60;
            
            let current_time_marker = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            current_time_marker.set_hexpand(true);
            let style_context = current_time_marker.style_context();
            let css = "box { background-color: rgba(255, 0, 0, 0.5); min-height: 2px; }";
            let provider = gtk::CssProvider::new();
            provider.load_from_data(css.as_bytes()).unwrap();
            style_context.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            
            meeting_area.put(&current_time_marker, 0, y_position);
        }

        // Group overlapping events
        let mut event_groups: Vec<Vec<&Event>> = Vec::new();
        for event in &events {
            if event.start_timestamp.time() == event.end_timestamp.time() {
                continue; // Skip all-day events
            }

            let mut found_group = false;
            for group in &mut event_groups {
                let overlaps = group.iter().any(|existing| {
                    !(event.end_timestamp <= existing.start_timestamp || 
                      event.start_timestamp >= existing.end_timestamp)
                });
                
                if overlaps {
                    group.push(event);
                    found_group = true;
                    break;
                }
            }
            
            if !found_group {
                event_groups.push(vec![event]);
            }
        }

        // Render event groups
        for group in event_groups {
            let group_size = group.len() as i32;
            let available_width = 600; // Will be adjusted based on actual width
            let button_width = ((available_width - (spacing * (group_size + 1))) / group_size).max(200);

            for (index, event) in group.iter().enumerate() {
                let event_start = event.start_timestamp.with_timezone(&Local);
                let event_end = event.end_timestamp.with_timezone(&Local);
                
                // Calculate position
                let start_minutes = (event_start.hour() as i32 - start_hour) * 60 + event_start.minute() as i32;
                let duration_minutes = event_end.signed_duration_since(event_start).num_minutes() as i32;
                
                let y_position = (start_minutes * hour_height) / 60;
                let height = (duration_minutes * hour_height) / 60;
                let x_position = spacing + (button_width + spacing) * index as i32;

                // Create event button
                let button = gtk::Button::new();
                button.set_size_request(button_width, height.max(30));

                // Style based on event status
                let now = Local::now();
                let style_context = button.style_context();
                let color = if now >= event.start_timestamp && now <= event.end_timestamp {
                    "rgba(100, 150, 255, 0.9)"  // Current - blue
                } else if now < event.start_timestamp {
                    "rgba(150, 200, 150, 0.9)"  // Upcoming - green
                } else {
                    "rgba(220, 220, 220, 0.9)"  // Past - gray
                };

                let css = format!(
                    "button {{ background: {}; border-radius: 4px; }}",
                    color
                );
                let provider = gtk::CssProvider::new();
                provider.load_from_data(css.as_bytes()).unwrap();
                style_context.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

                // Add event text
                let time_str = format!(
                    "{} - {} ({}m)",
                    event_start.format("%H:%M"),
                    event_end.format("%H:%M"),
                    duration_minutes
                );
                let label = gtk::Label::new(Some(&format!("{} - {}", time_str, event.summary)));
                label.set_line_wrap(true);
                label.set_line_wrap_mode(gtk::pango::WrapMode::WordChar);
                label.set_justify(gtk::Justification::Left);
                label.set_xalign(0.0);
                label.set_margin_start(8);
                label.set_margin_end(8);
                label.set_margin_top(4);
                label.set_margin_bottom(4);
                button.add(&label);

                // Add click handler
                if let Some(meet_url) = &event.meeturl {
                    let url = meet_url.clone();
                    button.connect_clicked(move |_| {
                        open_meeting(&url);
                    });
                }

                meeting_area.put(&button, x_position, y_position);
            }
        }

        // Assemble the layout
        layout_box.pack_start(&time_column, false, false, 0);
        layout_box.pack_start(&meeting_area, true, true, 0);

        // Set a minimum height for the layout
        let total_height = (end_hour - start_hour + 1) * hour_height;
        layout_box.set_size_request(-1, total_height);

        // Add to scrolled window
        let scrolled_window = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scrolled_window.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scrolled_window.add(&layout_box);
        container.pack_start(&scrolled_window, true, true, 0);

        Self { container, events }
    }
}

fn create_meetings_window(events: &[domain::Event]) -> gtk::Window {
    // Constants for calculating window size
    let start_hour = 7;
    let end_hour = 20;
    let base_height = 20;
    let window_height = (end_hour - start_hour + 1) * base_height * 4 + 50; // Add padding for decorations

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Today's Meetings");
    window.set_default_size(800, window_height);

    let main_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    main_box.set_margin_start(12);
    main_box.set_margin_end(12);
    main_box.set_margin_top(12);
    main_box.set_margin_bottom(12);

    // Add timeline view
    let timeline = TimelineView::new(events.to_vec());
    main_box.pack_start(&timeline.container, true, true, 0);

    window.add(&main_box);
    window.show_all();
    window
}

fn create_indicator_menu(events: &[domain::Event], indicator: &mut AppIndicator) {
    let mut m: Menu = gtk::Menu::new();
    let mut nof_upcoming_meetings = 0;
    if events.is_empty() {
        let item = gtk::MenuItem::with_label("test");
        let label = item.child().unwrap();
        (label.downcast::<gtk::Label>())
            .unwrap()
            .set_markup("<b>No Events Today</b>");
        m.append(&item);
    } else {
        for event in events {
            let all_day = event.start_timestamp.time() == event.end_timestamp.time();
            let time_string = if all_day {
                "All Day".to_owned()
            } else {
                format!(
                    "{} - {}",
                    &event.start_timestamp.format("%H:%M"),
                    &event.end_timestamp.format("%H:%M")
                )
                .to_owned()
            };
            let meeturl_string = match &event.meeturl {
                Some(_) => " (Zoom)",
                None => "",
            };

            let item = gtk::MenuItem::with_label("Test");
            let label = item.child().unwrap().downcast::<gtk::Label>().unwrap();
            let now = Local::now();
            let label_string = if all_day {
                format!("{}: {}{}", time_string, &event.summary, meeturl_string)
            } else if now < event.start_timestamp {
                nof_upcoming_meetings += 1;
                format!("◦ {}: {}{}", time_string, &event.summary, meeturl_string)
            } else if now >= event.start_timestamp && now <= event.end_timestamp {
                nof_upcoming_meetings += 1;
                format!("• {}: {}{}", time_string, &event.summary, meeturl_string)
            } else {
                format!("✓ {}: {}{}", time_string, &event.summary, meeturl_string)
            };

            label.set_text(&label_string);
            let new_event = (*event).clone();
            if new_event.meeturl.is_some() {
                item.connect_activate(move |_clicked_item| {
                    let meet_url = &new_event.meeturl.as_ref().unwrap();
                    open_meeting(meet_url);
                });
            }
            m.append(&item);
        }
    }
    
    // Add "Show Meetings Window" option
    let show_window_item = gtk::MenuItem::with_label("Show Meetings Window");
    let events_clone = events.to_vec();
    show_window_item.connect_activate(move |_| {
        create_meetings_window(&events_clone);
    });
    m.append(&gtk::SeparatorMenuItem::new());
    m.append(&show_window_item);
    
    let mi = gtk::MenuItem::with_label("Quit");
    mi.connect_activate(|_| {
        gtk::main_quit();
    });
    m.append(&gtk::SeparatorMenuItem::new());
    m.append(&mi);
    m.show_all();
    if nof_upcoming_meetings > 0 {
        println!("some meetings upcoming");
        set_some_meetings_left_icon(indicator);
    } else {
        println!("NO meetings upcoming");
        set_no_meetings_left_icon(indicator);
    }
    indicator.set_menu(&mut m);
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

fn show_event_notification(event: Event) {
    // println!("Event notification: {:?}", event);
    let summary_str = &format!(
        "{} - {}",
        event.start_timestamp.format("%H:%M"),
        event.summary
    );
    let mut notification = Notification::new();
    notification
        .summary(summary_str)
        .body(
            &event
                .meeturl
                .clone()
                .or_else(|| Some("No Zoom Meeting".to_string()))
                .unwrap(),
        )
        // icons are standard freedesktop.org icon names, see https://specifications.freedesktop.org/icon-naming-spec/icon-naming-spec-latest.html
        .icon("appointment-new")
        // Critical urgency has to be manually dismissed (according to XDG spec), this seems like what we want?
        .urgency(notify_rust::Urgency::Critical);
    // In case we have a meeting url we want to allow opening the meeting
    if let Some(meeturl) = event.meeturl {
        notification
            .action(
                &format!("{}{}", MEETERS_NOTIFICATION_ACTION_OPEN_MEETING, meeturl),
                "Open Zoom Meeting",
            )
            .show()
            .unwrap()
            .wait_for_action(|action| {
                if let Some(meeting) = action.strip_prefix(MEETERS_NOTIFICATION_ACTION_OPEN_MEETING)
                {
                    open_meeting(meeting);
                }
            });
    } else {
        if let Err(_) = notification.show() {
            println!("Could not show notification");
        }
    }
}

/// Time between two ical calendar download in milliseconds
const DEFAULT_POLLING_INTERVAL_MS: u128 = 2 * 60 * 1000;
/// The amount of time in seconds we want to be warned before the meeting starts
const DEFAULT_EVENT_WARNING_TIME_SECONDS: i64 = 60;
/// This is a prefix used to identify notification actions that are meant to open a meeting
const MEETERS_NOTIFICATION_ACTION_OPEN_MEETING: &str = "meeters_open_meeting:";

enum CalendarMessages {
    TodayEvents(Vec<Event>),
    EventNotification(Event),
}

fn default_tz(_: dotenvy::Error) -> Result<String, dotenvy::Error> {
    Ok("Europe/Berlin".to_string())
}

fn main() -> std::io::Result<()> {
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
    println!("Local Timezone configured as {}", local_tz_iana.clone());
    // magic incantation for gtk
    gtk::init().unwrap();
    // I can't get styles to work in appindicators
    // // Futzing with styles
    // let style = "label { color: red; }";
    // let provider = CssProvider::new();
    // provider.load_from_data(style.as_ref()).unwrap();
    // gtk::StyleContext::add_provider_for_screen(
    //     &gdk::Screen::get_default().expect("Error initializing gtk css provider."),
    //     &provider,
    //     gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    // );
    // set up our widgets
    let mut indicator = create_indicator();
    create_indicator_menu(&[], &mut indicator);

    // Create a message passing channel so we can communicate safely with the main GUI thread from our worker thread
    // let (status_sender, status_receiver) = glib::MainContext::channel::<String>(glib::PRIORITY_DEFAULT);
    let (events_sender, events_receiver) =
        glib::MainContext::channel::<Result<CalendarMessages, ()>>(glib::PRIORITY_DEFAULT);
    events_receiver.attach(None, move |event_result| {
        match event_result {
            Ok(TodayEvents(events)) => {
                if events.is_empty() {
                    create_indicator_menu(&[], &mut indicator);
                } else {
                    create_indicator_menu(&events, &mut indicator);
                }
            }
            Ok(EventNotification(event)) => {
                if config_show_event_notification {
                    show_event_notification(event);
                }
            }
            Err(_) => set_error_icon(&mut indicator),
        }
        glib::Continue(true)
    });
    // start the background thread for calendar work
    // this thread spawn here is inline because if I use another method I have trouble matching the lifetimes
    // (it requires static for the status_sender and I can't make that work yet)
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
                        // let local_date = Local::now().date() - chrono::Duration::days(6);
                        let local_date = Local::now().date();
                        let today_start = local_tz
                            .ymd(local_date.year(), local_date.month(), local_date.day())
                            .and_hms(0, 0, 0);
                        let today_end = local_tz
                            .ymd(local_date.year(), local_date.month(), local_date.day())
                            .and_hms(23, 59, 59);
                        let today_events = get_events_for_interval(events, today_start, today_end);
                        println!(
                            "There are {} events for today: {:?}",
                            today_events.len(),
                            today_events
                        );
                        last_events = today_events.clone();
                        events_sender
                            .send(Ok(TodayEvents(today_events)))
                            .expect("Channel should be sendable");
                    }
                    Err(e) => {
                        // TODO: maybe implement logging to some standard dir location and return more of an error for a tooltip
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
    // start listening for messages
    gtk::main();
    Ok(())
}
