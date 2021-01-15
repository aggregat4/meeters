use std::env;
use std::path::Path;
use std::thread;

use chrono::prelude::*;
use directories::ProjectDirs;
use gtk::prelude::*;
use gtk::Menu;
use libappindicator::{AppIndicator, AppIndicatorStatus};

use domain::CalendarError;
use std::time::{SystemTime, UNIX_EPOCH};

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
                    match gtk::show_uri(
                        None,
                        &new_event.meeturl.as_ref().unwrap(),
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .expect("Time must flow")
                            .as_secs() as u32,
                    ) {
                        Ok(_) => (),
                        Err(e) => eprintln!("Error trying to open the meeting URL: {}", e),
                    }
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
        glib::MainContext::channel::<Result<Vec<domain::Event>, ()>>(glib::PRIORITY_DEFAULT);
    events_receiver.attach(None, move |event_result| {
        match event_result {
            Ok(events) => {
                indicator.set_icon_full("meeters-appindicator", "icon");
                // TODO: update the menu to reflect all the events or that we have no events
                if events.is_empty() {
                    indicator.set_menu(&mut create_indicator_menu(&[]));
                } else {
                    indicator.set_menu(&mut create_indicator_menu(&events));
                }
            }
            Err(_) => indicator.set_icon_full("meeters-appindicator-error", "icon"),
        }
        glib::Continue(true)
    });
    // start the background thread for calendar work
    // this thread spawn here is inline because if I use another method I have trouble matching the lifetimes
    // (it requires static for the status_sender and I can't make that work yet)
    thread::spawn(move || loop {
        match get_ical(&ical_url).and_then(|t| meeters_ical::extract_events(&t)) {
            Ok(events) => {
                println!("Successfully got {:?} events", events.len());
                // let today_start = Local::now().date().and_hms(0, 0, 0) + chrono::Duration::days(2);
                // let today_end = Local::now().date().and_hms(23, 59, 59) + chrono::Duration::days(2);
                let today_start = Local::now().date().and_hms(0, 0, 0);
                let today_end = Local::now().date().and_hms(23, 59, 59);
                let mut today_events = events
                    .into_iter()
                    .filter(|e| e.start_timestamp > today_start && e.start_timestamp < today_end)
                    .collect::<Vec<_>>();
                today_events.sort_by(|a, b| Ord::cmp(&a.start_timestamp, &b.start_timestamp));
                println!(
                    "There are {} events for today: {:?}",
                    today_events.len(),
                    today_events
                );
                events_sender
                    .send(Ok(today_events))
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
        thread::sleep(std::time::Duration::from_secs(60 * 2));
    });
    // start listening for messages
    gtk::main();
    Ok(())
}
