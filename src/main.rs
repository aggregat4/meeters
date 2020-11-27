use std::env;
use std::path::Path;
use std::thread;
use std::time::Duration;
use gtk::prelude::*;
use libappindicator::{AppIndicator, AppIndicatorStatus};
use chrono::prelude::*;

use glib::{MainContext, Sender, Receiver};

mod meeters_ical;
mod domain;

use domain::CalendarError;

fn get_ical(url: &str) -> Result<String, CalendarError> {
    let response = ureq::get(url).call();
    if let Some(error) = response.synthetic_error() {
        return Err(CalendarError { msg: format!("Error getting ical from url: {}", error)});
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
    //indicator.set_label("Next meeting foobar 4 min", "8.8"); // does not get shown in XFCE
    //indicator.connect_activate
    indicator
}

fn create_indicator_menu() -> gtk::Menu {
    let m = gtk::Menu::new();
    let mi = gtk::MenuItem::with_label("Quit");
    mi.connect_activate(|_| {
        gtk::main_quit();
    });
    m.append(&mi);
    m.show_all();
    m
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
    
    let (sender, receiver) = glib::MainContext::channel::<String>(glib::PRIORITY_DEFAULT);
    // TODO: maybe use something more structure than strings here, maybe an enum?
    receiver.attach(None, move |text| {
        if text == "appindicator-error" {
            indicator.set_icon_full("meeters-appindicator-error", "icon");
        } else if text == "appindicator-noerror" {
            indicator.set_icon_full("meeters-appindicator", "icon");
        }
        glib::Continue(true)
    });
    // this thread spawn here is inline because if I use another method I have trouble matching the lifetimes
    // (it requires static for the sender and I can't make that work yet)
    thread::spawn(move || loop {
        match get_ical(&String::from(&args[1])).and_then(|t| meeters_ical::parse_events(&t)) {
            Ok(events) => {
                sender.send("appindicator-noerror".to_string()).expect("Channel should be sendable");
                println!("Successfully got {:?} events", events.len());
                let today_start = Local::now().date().and_hms(0, 0, 0);
                let today_end = Local::now().date().and_hms(23, 59, 59);
                let today_events = events
                    .into_iter()
                    .filter(|e| e.start_timestamp > today_start && e.start_timestamp < today_end)
                    .collect::<Vec<_>>();
                println!("There are {} events for today: {:?}", today_events.len(), today_events);
            },
            Err(e) => {
                sender.send("appindicator-error".to_string()).expect("Channel should be sendable");
                println!("Error getting events: {:?}", e.msg);
            }
        }
        thread::sleep(Duration::from_secs(10));
    });
    // start listening for messsages
    gtk::main();
}
