use std::env;
use std::path::Path;
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

mod chrono_ical;
mod chrono_windows_timezones;
mod domain;
mod meeters_ical;

fn get_ical(url: &str) -> Result<String, CalendarError> {
    let response = ureq::get(url).call();
    if let Some(error) = response.synthetic_error() {
        return Err(CalendarError {
            msg: format!("Error getting ical from url: {}", error),
        });
    }
    Ok(response.into_string().unwrap())
}

fn create_indicator() -> AppIndicator {
    let mut indicator = AppIndicator::new("rs-meetings", "");
    indicator.set_status(AppIndicatorStatus::Active);
    // including resources into a package is unsolved, except perhaps for something like https://doc.rust-lang.org/std/macro.include_bytes.html
    // for our purposes this should probably be a resource in the configuration somewhere
    let icon_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    indicator.set_icon_theme_path(icon_path.to_str().unwrap());
    indicator.set_icon_full("meeters-appindicator", "icon");
    indicator
}

fn open_meeting(meet_url: &str) {
    match gtk::show_uri(
        None,
        meet_url,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time must flow")
            .as_secs() as u32,
    ) {
        Ok(_) => (),
        Err(e) => eprintln!("Error trying to open the meeting URL: {}", e),
    }
}

fn create_indicator_menu(events: &[domain::Event]) -> gtk::Menu {
    let m: Menu = gtk::Menu::new();
    if events.is_empty() {
        let item = gtk::MenuItem::with_label("test");
        let label = item.get_child().unwrap();
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
        let now = Local::now();
        for event in events {
            let time_string = if event.start_timestamp.time() == event.end_timestamp.time() {
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
            let label_string = if now > event.start_timestamp {
                format!("{}: {}{}", time_string, &event.summary, meeturl_string)
            } else {
                format!(
                    "{}: <b>{}</b>{}",
                    time_string, &event.summary, meeturl_string
                )
            };
            // We need to actually create a menu item with a dummy label, then get that child
            // element, cast it to an actual label and then modify its markup to make sure we get
            // menu items that are left aligned but expand to fill horizontal space
            // The first attempt to create an empty item and then add a label caused those items
            // to have text that was only selectable/highlighted until the end of the text but not
            // the end of the menu item
            let item = gtk::MenuItem::with_label("Test");
            let label = item.get_child().unwrap().downcast::<gtk::Label>().unwrap();
            label.set_markup(&label_string);
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

fn load_config() -> std::io::Result<()> {
    let proj_dirs = ProjectDirs::from("net", "aggregat4", "meeters")
        .expect("Project directory must be available");
    let config_file = proj_dirs.config_dir().join("meeters_config.env");
    if !config_file.exists() {
        //fs::create_dir_all(config_dir)?;
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
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
) -> Vec<Event> {
    let mut filtered_events = events
        .into_iter()
        .filter(|e| e.start_timestamp > start_time && e.start_timestamp < end_time)
        .collect::<Vec<_>>();
    filtered_events.sort_by(|a, b| Ord::cmp(&a.start_timestamp, &b.start_timestamp));
    filtered_events
}

fn show_event_notification(event: Event) {
    println!("Event notification: {:?}", event);
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
                if action.starts_with(MEETERS_NOTIFICATION_ACTION_OPEN_MEETING) {
                    open_meeting(&action[MEETERS_NOTIFICATION_ACTION_OPEN_MEETING.len()..]);
                }
            });
    } else {
        // TODO: ignores error
        notification.show();
    }
}

/// Time between two ical calendar download in milliseconds
/// TODO: should be config
const POLLING_INTERVAL_MS: u128 = 2 * 60 * 1000;

/// The amount of time in seconds we want to be warned before the meeting starts
/// TODO: should be config
const EVENT_WARNING_TIME_SECONDS: i64 = 60;

/// This is a prefix used to identify notification actions that are meant to open a meeting
const MEETERS_NOTIFICATION_ACTION_OPEN_MEETING: &str = "meeters_open_meeting:";

enum CalendarMessages {
    TodayEvents(Vec<Event>),
    EventNotification(Event),
}

fn main() -> std::io::Result<()> {
    load_config()?;
    let ical_url =
        dotenv::var("ICAL_URL").expect("Expecting a configuration property with name ICAL_URL");
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
                indicator.set_icon_full("meeters-appindicator", "icon");
                // TODO: update the menu to reflect all the events or that we have no events
                if events.is_empty() {
                    indicator.set_menu(&mut create_indicator_menu(&[]));
                } else {
                    indicator.set_menu(&mut create_indicator_menu(&events));
                }
            }
            Ok(EventNotification(event)) => {
                show_event_notification(event);
            }
            Err(_) => indicator.set_icon_full("meeters-appindicator-error", "icon"),
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
            if last_download_time == 0 || current_time - last_download_time > POLLING_INTERVAL_MS {
                last_download_time = current_time;
                match get_ical(&ical_url).and_then(|t| meeters_ical::extract_events(&t)) {
                    Ok(events) => {
                        println!("Successfully got {:?} events", events.len());
                        // let today_start = Local::now().date().and_hms(0, 0, 0) + chrono::Duration::days(2);
                        // let today_end = Local::now().date().and_hms(23, 59, 59) + chrono::Duration::days(2);
                        let today_start = Local::now().date().and_hms(0, 0, 0);
                        let today_end = Local::now().date().and_hms(23, 59, 59);
                        // let today_start = Local::now().date().and_hms(0, 0, 0) - chrono::Duration::days(2);
                        // let today_end = Local::now().date().and_hms(23, 59, 59) - chrono::Duration::days(2);
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
                        println!("Error getting events: {:?}", e.msg);
                    }
                }
            }
            // Phase two of the background loop: check whether we have events that are close to occurring and trigger a notification
            // find the first event that is about to start in the next minute and if we did not notify before, send a notification
            let now = Local::now();
            let potential_next_immediate_upcoming_event = last_events.iter().find(|event| {
                let time_distance_from_now = event.start_timestamp.signed_duration_since(now);
                time_distance_from_now.num_seconds() > 0
                    && time_distance_from_now.num_seconds() <= EVENT_WARNING_TIME_SECONDS
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
                        Some(next_immediate_upcoming_event.start_timestamp.clone());
                }
            }
            thread::sleep(std::time::Duration::from_secs(5));
        }
    });
    // start listening for messages
    gtk::main();
    Ok(())
}
