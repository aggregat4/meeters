use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use chrono::prelude::*;
use dbus::blocking::Connection;
use dbus_crossroads::Crossroads;
use directories::ProjectDirs;
use gtk::prelude::*;
use gtk::Menu;
use libappindicator::{AppIndicator, AppIndicatorStatus};
use notify_rust::Notification;

use crate::domain::Event;

const HOUR_HEIGHT: i32 = 80; // Height for one hour

pub fn has_icons(dir: &Path) -> bool {
    let normal_icon_path = dir.with_file_name("meeters-appindicator.png");
    let error_icon_path = dir.with_file_name("meeters-appindicator-error.png");
    normal_icon_path.exists() && error_icon_path.exists()
}

pub fn find_icon_path() -> Option<PathBuf> {
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

pub fn set_error_icon(indicator: &mut AppIndicator) {
    if let Some(icon_path) = find_icon_path() {
        indicator.set_icon(
            icon_path
                .with_file_name("meeters-appindicator-error.png")
                .to_str()
                .unwrap(),
        );
    }
}

pub fn set_some_meetings_left_icon(indicator: &mut libappindicator::AppIndicator) {
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

pub fn set_no_meetings_left_icon(indicator: &mut libappindicator::AppIndicator) {
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
        icon_path.with_file_name("meeters-appindicator.png")
    } else {
        nomeetingsleft_icon_path
    }
}

pub fn create_indicator() -> AppIndicator {
    let mut indicator = AppIndicator::new("meeters", "");
    indicator.set_status(AppIndicatorStatus::Active);
    match find_icon_path() {
        Some(icon_path) => {
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
        }
    }
}

pub fn open_meeting(meet_url: &str) {
    match gtk::show_uri_on_window(None::<&gtk::Window>, meet_url, gtk::current_event_time()) {
        Ok(_) => (),
        Err(e) => eprintln!("Error trying to open the meeting URL: {}", e),
    }
}

pub struct TimelineView {
    pub container: gtk::Box,
}

impl TimelineView {
    fn create_event_button(event: &Event, width: i32, height: i32, show_time: bool) -> gtk::Button {
        let button = gtk::Button::new();
        button.set_size_request(width, height.max(30));

        // Add tooltip with event description
        let trimmed_description = event.description.trim();
        if !trimmed_description.is_empty() {
            button.set_tooltip_text(Some(trimmed_description));
        }

        // Style based on event status
        let now = Local::now();
        let style_context = button.style_context();
        let color = if now >= event.start_timestamp && now <= event.end_timestamp {
            "rgba(255, 165, 0, 0.6)" // Current - orange
        } else if now < event.start_timestamp {
            "rgba(180, 200, 255, 0.8)" // Upcoming - muted blue with higher opacity
        } else {
            "rgba(220, 220, 220, 0.6)" // Past - lighter gray
        };

        let css = format!("button {{ background: {}; border-radius: 4px; }}", color);
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
            format!(
                "{}  {}{}",
                time_str,
                event.summary,
                if event.meeturl.is_some() {
                    " (Zoom)"
                } else {
                    ""
                }
            )
        } else {
            format!(
                "{}{}",
                event.summary,
                if event.meeturl.is_some() {
                    " (Zoom)"
                } else {
                    ""
                }
            )
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

    pub fn new(events: Vec<Event>, start_hour: i32, end_hour: i32, is_today: bool) -> Self {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.set_margin_start(12);
        container.set_margin_end(12);
        container.set_margin_top(12);
        container.set_margin_bottom(12);

        // Separate all-day events from regular events
        let (all_day_events, regular_events): (Vec<_>, Vec<_>) = events
            .into_iter()
            .partition(|e| e.start_timestamp.time() == e.end_timestamp.time());

        // Create all-day events section - always show it for consistent spacing
        let all_day_container = gtk::Box::new(gtk::Orientation::Vertical, 6);
        all_day_container.set_margin_bottom(12);

        // Add "All Day" label
        let all_day_label = gtk::Label::new(Some("All Day"));
        all_day_label.set_xalign(0.0);
        all_day_label.set_margin_bottom(4);
        all_day_container.pack_start(&all_day_label, false, false, 0);

        // Create horizontal box for all-day events
        let all_day_events_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);

        // Set a minimum height for the all-day events box to ensure consistent spacing
        all_day_events_box.set_size_request(-1, 40);

        // Calculate button width based on number of events
        let available_width = 600; // Match the timeline width

        if !all_day_events.is_empty() {
            let button_width = ((available_width - (6 * (all_day_events.len() as i32 + 1)))
                / all_day_events.len() as i32)
                .max(150);

            for event in all_day_events {
                let button = Self::create_event_button(&event, button_width, 40, false);
                all_day_events_box.pack_start(&button, true, true, 0);
            }
        }

        all_day_container.pack_start(&all_day_events_box, false, false, 0);
        container.pack_start(&all_day_container, false, false, 0);

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

        // Current time indicator (only for today's view)
        if is_today {
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
        }

        // Group overlapping events
        let mut event_groups: Vec<Vec<&Event>> = Vec::new();
        for event in &regular_events {
            let mut found_group = false;
            for group in &mut event_groups {
                let overlaps = group.iter().any(|existing| {
                    !(event.end_timestamp <= existing.start_timestamp
                        || event.start_timestamp >= existing.end_timestamp)
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
            let button_width =
                ((available_width - (spacing * (group_size + 1))) / group_size).max(200);

            for (index, event) in group.iter().enumerate() {
                let event_start = event.start_timestamp.with_timezone(&Local);
                let event_end = event.end_timestamp.with_timezone(&Local);

                // Calculate position
                let start_minutes =
                    (event_start.hour() as i32 - start_hour) * 60 + event_start.minute() as i32;
                let duration_minutes =
                    event_end.signed_duration_since(event_start).num_minutes() as i32;

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

        container.pack_start(&layout_box, true, true, 0);

        Self { container }
    }
}

fn calculate_window_height(start_hour: i32, end_hour: i32) -> i32 {
    // Constants for calculating window size
    (end_hour - start_hour) * HOUR_HEIGHT + HOUR_HEIGHT + 90 // Add padding for decorations
}

pub struct WindowManager {
    pub current_window: Option<gtk::Window>,
    day_events: Arc<Mutex<Vec<Vec<Event>>>>,
    start_hour: i32,
    end_hour: i32,
    future_days: i32,
}

impl WindowManager {
    pub fn new(start_hour: i32, end_hour: i32, future_days: i32) -> Self {
        WindowManager {
            current_window: None,
            day_events: Arc::new(Mutex::new(Vec::new())),
            start_hour,
            end_hour,
            future_days,
        }
    }

    pub fn toggle_window(&mut self) {
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

    pub fn show_window(&mut self) {
        let day_events = self.day_events.lock().unwrap();

        if let Some(window) = &self.current_window {
            if window.is_visible() {
                window.present();
                return;
            }
        }

        // Create new window
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("Calendar View");
        window.set_default_size(
            (700 * (self.future_days + 1)) as i32,
            calculate_window_height(self.start_hour, self.end_hour),
        );

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        main_box.set_margin_start(6);
        main_box.set_margin_end(6);
        main_box.set_margin_top(6);
        main_box.set_margin_bottom(6);

        // Create a scrolled window that will contain all days
        let scrolled_window =
            gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scrolled_window.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);

        // Create horizontal box for all days
        let days_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);

        // Add each day's timeline
        for (day_index, events) in day_events.iter().enumerate() {
            let day_box = gtk::Box::new(gtk::Orientation::Vertical, 6);

            // Create day label
            let label_text = if day_index == 0 {
                "Today".to_string()
            } else if day_index == 1 {
                "Tomorrow".to_string()
            } else {
                let date = Local::now().date_naive() + chrono::Duration::days(day_index as i64);
                format!("{}", date.format("%A, %B %d"))
            };

            let day_label = gtk::Label::new(Some(&label_text));
            day_label.set_xalign(0.0);
            day_label.set_margin_bottom(4);
            day_box.pack_start(&day_label, false, false, 0);

            let timeline = TimelineView::new(
                events.clone(),
                self.start_hour,
                self.end_hour,
                day_index == 0,
            );
            day_box.pack_start(&timeline.container, true, true, 0);
            days_box.pack_start(&day_box, true, true, 0);
        }

        scrolled_window.add(&days_box);
        main_box.pack_start(&scrolled_window, true, true, 0);
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

    pub fn update_events(&mut self, new_events: Vec<Vec<Event>>) {
        // Update stored events
        let mut events = self.day_events.lock().unwrap();
        *events = new_events;

        // Update window if it exists
        if let Some(window) = &self.current_window {
            if let Some(main_box) = window.children().first() {
                let main_box = main_box.clone().downcast::<gtk::Box>().unwrap();
                main_box
                    .children()
                    .iter()
                    .for_each(|child| main_box.remove(child));

                // Create a scrolled window that will contain all days
                let scrolled_window =
                    gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
                scrolled_window.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);

                // Create horizontal box for all days
                let days_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);

                // Add each day's timeline
                for (day_index, day_events) in events.iter().enumerate() {
                    let day_box = gtk::Box::new(gtk::Orientation::Vertical, 6);

                    // Create day label
                    let label_text = if day_index == 0 {
                        "Today".to_string()
                    } else if day_index == 1 {
                        "Tomorrow".to_string()
                    } else {
                        let date =
                            Local::now().date_naive() + chrono::Duration::days(day_index as i64);
                        format!("{}", date.format("%A, %B %d"))
                    };

                    let day_label = gtk::Label::new(Some(&label_text));
                    day_label.set_xalign(0.0);
                    day_label.set_margin_bottom(4);
                    day_box.pack_start(&day_label, false, false, 0);

                    let timeline = TimelineView::new(
                        day_events.clone(),
                        self.start_hour,
                        self.end_hour,
                        day_index == 0,
                    );
                    day_box.pack_start(&timeline.container, true, true, 0);
                    days_box.pack_start(&day_box, true, true, 0);
                }

                scrolled_window.add(&days_box);
                main_box.pack_start(&scrolled_window, true, true, 0);
                main_box.show_all();
            }
        }
    }
}

pub fn create_indicator_menu(
    today_events: &[Event],
    indicator: &mut AppIndicator,
    window_manager: Arc<Mutex<WindowManager>>,
) {
    let mut m: Menu = gtk::Menu::new();
    let mut nof_upcoming_meetings = 0;

    if today_events.is_empty() {
        let item = gtk::MenuItem::with_label("test");
        let label = item.child().unwrap();
        (label.downcast::<gtk::Label>())
            .unwrap()
            .set_markup("<b>No Events Today</b>");
        m.append(&item);
    } else {
        for event in today_events {
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
                item.connect_activate(move |_| {
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

pub fn show_event_notification(event: Event) {
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
        .icon("appointment-new")
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
    } else if let Err(_) = notification.show() {
        println!("Could not show notification");
    }
}

/// This is a prefix used to identify notification actions that are meant to open a meeting
pub const MEETERS_NOTIFICATION_ACTION_OPEN_MEETING: &str = "meeters_open_meeting:";

fn get_config_directory() -> PathBuf {
    ProjectDirs::from("net", "aggregat4", "meeters")
        .expect("Project directory must be available")
        .config_dir()
        .to_path_buf()
}

pub fn initialize_gui(
    start_hour: i32,
    end_hour: i32,
    future_days: i32,
) -> (
    AppIndicator,
    Arc<Mutex<WindowManager>>,
    glib::Receiver<(String, ())>,
) {
    // Initialize GTK
    gtk::init().unwrap();

    // Create window manager
    let window_manager = Arc::new(Mutex::new(WindowManager::new(
        start_hour,
        end_hour,
        future_days,
    )));

    // Set up D-Bus connection
    let connection = Connection::new_session().expect("Failed to connect to D-Bus");
    connection
        .request_name("net.aggregat4.Meeters", false, true, false)
        .expect("Failed to request D-Bus name");

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
                show_sender.send(("show".to_string(), ())).unwrap();
                Ok(())
            });

            let close_sender = close_sender.clone();
            b.method("CloseWindow", (), (), move |_, _, ()| {
                close_sender.send(("close".to_string(), ())).unwrap();
                Ok(())
            });

            let toggle_sender = toggle_sender.clone();
            b.method("ToggleWindow", (), (), move |_, _, ()| {
                toggle_sender.send(("toggle".to_string(), ())).unwrap();
                Ok(())
            });
        })
    };

    cr.insert("/net/aggregat4/Meeters", &[iface_token], ());

    // Spawn D-Bus handler thread
    let cr_clone = cr;
    thread::spawn(move || {
        cr_clone.serve(&connection).unwrap();
    });

    // Create indicator
    let mut indicator = create_indicator();
    create_indicator_menu(&[], &mut indicator, Arc::clone(&window_manager));

    (indicator, window_manager, dbus_receiver)
}

pub fn run_gui_main_loop() {
    gtk::main();
}
