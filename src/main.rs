use lazy_static::lazy_static;
use regex::Regex;
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

use crate::domain::Event;
use crate::CalendarMessages::{EventNotification, TodayEvents};
use domain::CalendarError;
use std::time::{SystemTime, UNIX_EPOCH};

mod binary_search;
mod chrono_ical;
mod chrono_windows_timezones;
mod custom_timezone;
mod domain;
mod ical_util;
mod meeters_ical;

fn get_ical(url: &str) -> Result<String, CalendarError> {
    match ureq::get(url).call() {
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

fn set_success_icon(indicator: &mut libappindicator::AppIndicator) {
    if let Some(icon_path) = find_icon_path() {
        indicator.set_icon(
            icon_path
                .with_file_name("meeters-appindicator.png")
                .to_str()
                .unwrap(),
        );
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
    lazy_static! {
        static ref ZOOM_FULL_MEETING_REGEX: regex::Regex =
            Regex::new(r"https?://[^\s]*zoom.us/j/([0-9]+)(\?.*pwd=([A-Za-z0-9]+))?").unwrap();
    }
    // this is an attempt to transform a zoom http link to a zommtg: link
    // these links bypass opening a new browser tab and go into zoom directly
    // sadly we can't use this for personal meeting rooms though, only full meeting
    // IDs
    let final_url = if let Some(_) = ZOOM_FULL_MEETING_REGEX.find(meet_url) {
        let caps = ZOOM_FULL_MEETING_REGEX.captures(meet_url).unwrap();
        let meet_id = caps.get(1);
        let password = caps.get(3);
        if password.is_some() {
            format!(
                "zoommtg://zoom.us/join?confno={}&pwd={}",
                meet_id.unwrap().as_str(),
                password.unwrap().as_str()
            )
        } else {
            format!(
                "zoommtg://zoom.us/join?confno={}",
                meet_id.unwrap().as_str()
            )
        }
    } else {
        meet_url.to_string()
    };
    println!("final URL for meeting: {:?}", final_url);
    match gtk::show_uri(None, &final_url, gtk::current_event_time()) {
        Ok(_) => (),
        Err(e) => eprintln!("Error trying to open the meeting URL: {}", e),
    }
}

fn create_indicator_menu(events: &[domain::Event]) -> gtk::Menu {
    let m: Menu = gtk::Menu::new();
    if events.is_empty() {
        let item = gtk::MenuItem::with_label("test");
        let label = item.child().unwrap();
        (label.downcast::<gtk::Label>())
            .unwrap()
            .set_markup("<b>No Events Today</b>");
        // let label = Label::new(None);
        // label.set_markup("<b>No Events Today</b>");
        // let item = gtk::MenuItem::new();
        // // item.set_hexpand(true);
        // item.set_halign(gtk::Align::Start);
        // item.add(&label);
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

            // We need to actually create a menu item with a dummy label, then get that child
            // element, cast it to an actual label and then modify its markup to make sure we get
            // menu items that are left aligned but expand to fill horizontal space
            // The first attempt to create an empty item and then add a label caused those items
            // to have text that was only selectable/highlighted until the end of the text but not
            // the end of the menu item
            let item = gtk::MenuItem::with_label("Test");
            let label = item.child().unwrap().downcast::<gtk::Label>().unwrap();
            // we used to format this text with markup and uset set_markup but that causes potential
            // escaping issues and we just default to plain text now
            let now = Local::now();
            let label_string = if all_day {
                format!("{}: {}{}", time_string, &event.summary, meeturl_string)
            } else if now < event.start_timestamp {
                format!("⃝ {}: {}{}", time_string, &event.summary, meeturl_string)
            } else if now >= event.start_timestamp && now <= event.end_timestamp {
                format!("* {}: {}{}", time_string, &event.summary, meeturl_string)
            } else {
                format!("✓  {}: {}{}", time_string, &event.summary, meeturl_string)
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
    let mi = gtk::MenuItem::with_label("Quit");
    mi.connect_activate(|_| {
        gtk::main_quit();
    });
    m.append(&mi);
    m.show_all();
    m
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
    dotenv::from_path(config_file).expect("Can not load configuration file meeters_config.env");
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
                    println!("About to open {:?}", meeting);
                    open_meeting(meeting);
                }
            });
    } else {
        // TODO: ignores error
        notification.show();
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

fn main() -> std::io::Result<()> {
    load_config()?;
    // Parse config
    let config_ical_url = dotenv::var("MEETERS_ICAL_URL")
        .expect("Expecting a configuration property with name MEETERS_ICAL_URL");
    let config_show_event_notification: bool = match dotenv::var("MEETERS_EVENT_NOTIFICATION") {
        Ok(val) => val.parse::<bool>().expect(
            "Value for MEETERS_EVENT_NOTIFICATION configuration parameter must be a boolean",
        ),
        Err(_) => true,
    };
    let config_polling_interval_ms: u128 = match dotenv::var("MEETERS_POLLING_INTERVAL_MS") {
        Ok(val) => val.parse::<u128>().expect("MEETERS_POLLING_INTERVAL_MS must be a positive integer expressing the polling interval in milliseconds"),
        Err(_) => DEFAULT_POLLING_INTERVAL_MS
    };
    let config_event_warning_time_seconds: i64 = match dotenv::var("MEETERS_EVENT_WARNING_TIME_SECONDS") {
        Ok(val) => val.parse::<i64>().expect("MEETERS_EVENT_WARNING_TIME_SECONDS must be a positive integer expressing the polling interval in seconds"),
        Err(_) => DEFAULT_EVENT_WARNING_TIME_SECONDS
    };
    // magic incantation for gtk
    gtk::init().unwrap();
    // set up our widgets
    let mut indicator = create_indicator();
    let mut menu = create_indicator_menu(&[]);
    indicator.set_menu(&mut menu);
    // Create a message passing channel so we can communicate safely with the main GUI thread from our worker thread
    // let (status_sender, status_receiver) = glib::MainContext::channel::<String>(glib::PRIORITY_DEFAULT);
    let (events_sender, events_receiver) =
        glib::MainContext::channel::<Result<CalendarMessages, ()>>(glib::PRIORITY_DEFAULT);
    events_receiver.attach(None, move |event_result| {
        match event_result {
            Ok(TodayEvents(events)) => {
                set_success_icon(&mut indicator);
                // TODO: update the menu to reflect all the events or that we have no events
                if events.is_empty() {
                    indicator.set_menu(&mut create_indicator_menu(&[]));
                } else {
                    indicator.set_menu(&mut create_indicator_menu(&events));
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
                match get_ical(&config_ical_url).and_then(|t| meeters_ical::extract_events(&t)) {
                    Ok(events) => {
                        println!("Successfully got {:?} events", events.len());
                        // let local_date = Local::now().date() - chrono::Duration::days(6);
                        let local_date = Local::now().date();
                        let today_start = meeters_ical::LOCAL_TZ
                            .ymd(local_date.year(), local_date.month(), local_date.day())
                            .and_hms(0, 0, 0);
                        let today_end = meeters_ical::LOCAL_TZ
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
