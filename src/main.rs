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
use ureq::Agent;
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

use dbus::blocking::Connection;
use dbus_crossroads::Crossroads;
use std::sync::Arc;
use std::sync::Mutex;

fn get_ical(url: &str) -> Result<String, CalendarError> {
    println!("trying to fetch ical");
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build();
    let agent: Agent = config.into();
    return agent.get(url)
        .call()
        .map_err(|e| CalendarError {
            msg: format!("Error calling calendar URL: {}", e)
        })?
        .body_mut()
        .read_to_string()
        .map_err(|e| CalendarError {
            msg: format!("Error reading calendar response body: {}", e)
        });
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
    match gtk::show_uri_on_window(None::<&gtk::Window>, meet_url, gtk::current_event_time()) {
        Ok(_) => (),
        Err(e) => eprintln!("Error trying to open the meeting URL: {}", e),
    }
}

const HOUR_HEIGHT: i32 = 80;  // Height for one hour

struct TimelineView {
    container: gtk::Box,
}

impl TimelineView {
    fn create_event_button(event: &Event, width: i32, height: i32, show_time: bool) -> gtk::Button {
        let button = gtk::Button::new();
        button.set_size_request(width, height.max(30));

        // Style based on event status
        let now = Local::now();
        let style_context = button.style_context();
        let color = if now >= event.start_timestamp && now <= event.end_timestamp {
            "rgba(255, 165, 0, 0.6)"  // Current - orange
        } else if now < event.start_timestamp {
            "rgba(150, 180, 255, 0.6)"  // Upcoming - lighter blue
        } else {
            "rgba(220, 220, 220, 0.6)"  // Past - lighter gray
        };

        let css = format!(
            "button {{ background: {}; border-radius: 4px; }}",
            color
        );
        let provider = gtk::CssProvider::new();
        provider.load_from_data(css.as_bytes()).unwrap();
        style_context.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

        // Add event text
        let text = if show_time {
            let event_start = event.start_timestamp.with_timezone(&Local);
            let event_end = event.end_timestamp.with_timezone(&Local);
            let time_str = format!(
                "{} - {}",
                event_start.format("%H:%M"),
                event_end.format("%H:%M")
            );
            format!("{}  {}{}", time_str, event.summary, if event.meeturl.is_some() { " (Zoom)" } else { "" })
        } else {
            format!("{}{}", event.summary, if event.meeturl.is_some() { " (Zoom)" } else { "" })
        };

        let label = gtk::Label::new(Some(&text));
        label.set_line_wrap(true);
        label.set_line_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_justify(gtk::Justification::Left);
        label.set_xalign(0.0);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        button.add(&label);

        // Add click handler for meeting URL if available
        if let Some(meet_url) = &event.meeturl {
            let url = meet_url.clone();
            button.connect_clicked(move |_| {
                open_meeting(&url);
            });
        }

        button
    }

    fn new(events: Vec<Event>, start_hour: i32, end_hour: i32) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.set_margin_start(12);
        container.set_margin_end(12);
        container.set_margin_top(12);
        container.set_margin_bottom(12);

        // Separate all-day events from regular events
        let (all_day_events, regular_events): (Vec<_>, Vec<_>) = events
            .into_iter()
            .partition(|e| e.start_timestamp.time() == e.end_timestamp.time());

        // Create all-day events section if there are any
        if !all_day_events.is_empty() {
            let all_day_container = gtk::Box::new(gtk::Orientation::Vertical, 6);
            all_day_container.set_margin_bottom(12);

            // Add "All Day" label
            let all_day_label = gtk::Label::new(Some("All Day"));
            all_day_label.set_xalign(0.0);
            all_day_label.set_margin_bottom(4);
            all_day_container.pack_start(&all_day_label, false, false, 0);

            // Create horizontal box for all-day events
            let all_day_events_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            
            // Calculate button width based on number of events
            let available_width = 600; // Match the timeline width
            let button_width = ((available_width - (6 * (all_day_events.len() as i32 + 1))) / all_day_events.len() as i32).max(150);
            
            for event in all_day_events {
                let button = Self::create_event_button(&event, button_width, 40, false);
                all_day_events_box.pack_start(&button, true, true, 0);
            }

            all_day_container.pack_start(&all_day_events_box, false, false, 0);
            container.pack_start(&all_day_container, false, false, 0);
        }

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
            let y_position = (hour - start_hour) * HOUR_HEIGHT;
            
            // Hour label
            let label = gtk::Label::new(Some(&format!("{:02}:00", hour)));
            label.set_xalign(1.0);
            label.set_margin_end(5);
            time_column.put(&label, 0, y_position);

            // Hour separator with styling
            let separator = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            separator.set_size_request(600, -1); // Explicit width, slightly less than window width
            let style_context = separator.style_context();
            
            // Different styles for start/end of day vs regular hours
            let css = if hour == start_hour || hour == end_hour {
                "box { background-color: rgba(100, 100, 100, 0.3); min-height: 2px; margin: 0; padding: 0; }"
            } else {
                "box { background-color: rgba(200, 200, 200, 0.3); min-height: 1px; margin: 0; padding: 0; }"
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
            let y_position = (minutes_from_start * HOUR_HEIGHT) / 60;
            
            let current_time_marker = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            current_time_marker.set_size_request(600, -1); // Match separator width
            let style_context = current_time_marker.style_context();
            let provider = gtk::CssProvider::new();
            provider
                .load_from_data(b"box { background-color: rgba(255, 0, 0, 0.6); min-height: 2px; margin: 0; padding: 0; }")
                .unwrap();
            style_context.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            
            meeting_area.put(&current_time_marker, 0, y_position);
        }

        // Group overlapping events
        let mut event_groups: Vec<Vec<&Event>> = Vec::new();
        for event in &regular_events {
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
                
                let y_position = (start_minutes * HOUR_HEIGHT) / 60;
                let height = (duration_minutes * HOUR_HEIGHT) / 60;
                let x_position = spacing + (button_width + spacing) * index as i32;

                let button = Self::create_event_button(event, button_width, height, true);
                meeting_area.put(&button, x_position, y_position);
            }
        }

        // Assemble the layout
        layout_box.pack_start(&time_column, false, false, 0);
        layout_box.pack_start(&meeting_area, true, true, 0);

        // Set a minimum height for the layout - no need for +1 as we want to end exactly at end_hour
        let total_height = (end_hour - start_hour) * HOUR_HEIGHT;
        layout_box.set_size_request(-1, total_height);

        // Add to scrolled window
        let scrolled_window = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scrolled_window.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scrolled_window.add(&layout_box);
        container.pack_start(&scrolled_window, true, true, 0);

        Self { container }
    }
}


fn calculate_window_height(start_hour: i32, end_hour: i32) -> i32 {
    // Constants for calculating window size
    (end_hour - start_hour) * HOUR_HEIGHT + HOUR_HEIGHT + 90 // Add padding for decorations
}

struct WindowManager {
    current_window: Option<gtk::Window>,
    events: Arc<Mutex<Vec<domain::Event>>>,
    start_hour: i32,
    end_hour: i32,
}

impl WindowManager {
    fn new(start_hour: i32, end_hour: i32) -> Self {
        WindowManager {
            current_window: None,
            events: Arc::new(Mutex::new(Vec::new())),
            start_hour,
            end_hour,
        }
    }

    fn toggle_window(&mut self) {
        if let Some(window) = &self.current_window {
            if window.is_visible() {
                window.hide();
            } else {
                window.present();
            }
        } else {
            self.show_window();
        }
    }

    fn show_window(&mut self) {
        let events = self.events.lock().unwrap();
        
        if let Some(window) = &self.current_window {
            if window.is_visible() {
                window.present();
                return;
            }
        }
        
        // Create new window
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("Today's Meetings");
        window.set_default_size(700, calculate_window_height(self.start_hour, self.end_hour));

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        main_box.set_margin_start(6);
        main_box.set_margin_end(6);
        main_box.set_margin_top(6);
        main_box.set_margin_bottom(6);

        // Add timeline view
        let timeline = TimelineView::new(events.to_vec(), self.start_hour, self.end_hour);
        main_box.pack_start(&timeline.container, true, true, 0);

        window.add(&main_box);
        
        // Handle window close
        let window_clone = window.clone();
        window.connect_delete_event(move |_, _| {
            window_clone.hide();
            glib::Propagation::Stop
        });
        
        window.show_all();
        self.current_window = Some(window);
    }

    fn update_events(&mut self, new_events: Vec<domain::Event>) {
        // Update stored events
        let mut events = self.events.lock().unwrap();
        *events = new_events;
        
        // Update window if it exists
        if let Some(window) = &self.current_window {
            if let Some(main_box) = window.children().first() {
                let main_box = main_box.clone().downcast::<gtk::Box>().unwrap();
                main_box.children().iter().for_each(|child| main_box.remove(child));
                
                let timeline = TimelineView::new(events.to_vec(), self.start_hour, self.end_hour);
                main_box.pack_start(&timeline.container, true, true, 0);
                main_box.show_all();
            }
        }
    }

}

fn create_indicator_menu(events: &[domain::Event], indicator: &mut AppIndicator, window_manager: Arc<Mutex<WindowManager>>) {
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
    let window_manager_clone = Arc::clone(&window_manager);
    show_window_item.connect_activate(move |_| {
        let mut wm = window_manager_clone.lock().unwrap();
        wm.show_window();
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
/// Default start hour for the timeline view (8 AM)
const DEFAULT_START_HOUR: i32 = 8;
/// Default end hour for the timeline view (8 PM)
const DEFAULT_END_HOUR: i32 = 20;

enum CalendarMessages {
    TodayEvents(Vec<Event>),
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
    println!("Local Timezone configured as {}", local_tz_iana.clone());


    // Set up D-Bus connection
    let connection = Connection::new_session().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    connection.request_name("net.aggregat4.Meeters", false, true, false)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    // Create window manager
    let window_manager = Arc::new(Mutex::new(WindowManager::new(config_start_hour, config_end_hour)));
    
    // Create a channel for D-Bus requests
    let (dbus_sender, dbus_receiver) = glib::MainContext::channel(glib::Priority::DEFAULT);
    
    // Create D-Bus interface
    let mut cr = Crossroads::new();
    
    let iface_token = {
        let show_sender = dbus_sender.clone();
        let close_sender = dbus_sender.clone();
        let toggle_sender = dbus_sender.clone();
        
        cr.register("net.aggregat4.Meeters", move |b| {
            let show_sender = show_sender.clone();
            b.method("ShowWindow", (), (), move |_, _, ()| {
                show_sender.send(("show", ())).unwrap();
                Ok(())
            });
            
            let close_sender = close_sender.clone();
            b.method("CloseWindow", (), (), move |_, _, ()| {
                close_sender.send(("close", ())).unwrap();
                Ok(())
            });

            let toggle_sender = toggle_sender.clone();
            b.method("ToggleWindow", (), (), move |_, _, ()| {
                toggle_sender.send(("toggle", ())).unwrap();
                Ok(())
            });
        })
    };
    
    cr.insert("/net/aggregat4/Meeters", &[iface_token], ());
    
    // Handle D-Bus requests in the main GTK thread
    let window_manager_clone = Arc::clone(&window_manager);
    dbus_receiver.attach(None, move |(action, _)| {
        let mut wm = window_manager_clone.lock().unwrap();
        match action {
            "show" => wm.show_window(),
            "close" => {
                if let Some(window) = &wm.current_window {
                    window.hide();
                }
            },
            "toggle" => wm.toggle_window(),
            _ => (),
        }
        glib::ControlFlow::Continue
    });
    
    // Spawn D-Bus handler thread
    let cr_clone = cr;
    thread::spawn(move || {
        cr_clone.serve(&connection).unwrap();
    });


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
    create_indicator_menu(&[], &mut indicator, Arc::clone(&window_manager));

    // Create a message passing channel so we can communicate safely with the main GUI thread from our worker thread
    // let (status_sender, status_receiver) = glib::MainContext::channel::<String>(glib::PRIORITY_DEFAULT);
    let (events_sender, events_receiver) =
        glib::MainContext::channel::<Result<CalendarMessages, ()>>(glib::Priority::DEFAULT);
    events_receiver.attach(None, move |event_result| {
        match event_result {
            Ok(TodayEvents(events)) => {
                // Update window manager with new events
                let mut wm = window_manager.lock().unwrap();
                wm.update_events(events.clone());
                
                if events.is_empty() {
                    create_indicator_menu(&[], &mut indicator, Arc::clone(&window_manager));
                } else {
                    create_indicator_menu(&events, &mut indicator, Arc::clone(&window_manager));
                }
            }
            Ok(EventNotification(event)) => {
                if config_show_event_notification {
                    show_event_notification(event);
                }
            }
            Err(_) => set_error_icon(&mut indicator),
        }
        glib::ControlFlow::Continue
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
