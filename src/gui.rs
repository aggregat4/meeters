use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use async_channel::Receiver;
use chrono::prelude::*;
use dbus::blocking::Connection;
use dbus_crossroads::Crossroads;
use directories::ProjectDirs;
use gtk::prelude::*;
use gtk::Menu;
use libappindicator::{AppIndicator, AppIndicatorStatus};
use notify_rust::Notification;

use crate::domain::{Event, RefreshState};

const HOUR_HEIGHT: i32 = 80; // Height for one hour
const TIMELINE_MIN_WIDTH: i32 = 600;
const DAY_MIN_WIDTH: i32 = 700;

const TEXT_PRIMARY: &str = "#242a31";
const TEXT_SUBTLE: &str = "#75808c";
const TIMELINE_BACKGROUND: &str = "#fbfaf7";
const TIMELINE_GRID: &str = "rgba(74, 83, 94, 0.14)";
const TIMELINE_GRID_STRONG: &str = "rgba(74, 83, 94, 0.28)";
const TIMELINE_RAIL: &str = "rgba(74, 83, 94, 0.18)";
const CURRENT_TIME_MARKER: &str = "rgba(218, 55, 48, 0.72)";

struct EventPalette {
    background: &'static str,
    border: &'static str,
    text: &'static str,
}

fn load_css(style_context: &gtk::StyleContext, css: &str) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(css.as_bytes()).unwrap();
    style_context.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

fn style_label(label: &gtk::Label, color: &str) {
    style_label_with_css(label, color, "");
}

fn style_label_with_css(label: &gtk::Label, color: &str, extra_css: &str) {
    load_css(
        &label.style_context(),
        &format!(
            "label {{ color: {}; text-shadow: none; {} }}",
            color, extra_css
        ),
    );
}

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
        Err(e) => log::error!("error trying to open the meeting URL: {}", e),
    }
}

fn format_refresh_timestamp(timestamp: Option<DateTime<Local>>) -> String {
    timestamp
        .map(|ts| ts.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "never".to_string())
}

fn refresh_status_menu_label(refresh_state: &RefreshState) -> String {
    match refresh_state.last_update_successful {
        Some(true) => format!(
            "Last update: {} (successful)",
            format_refresh_timestamp(refresh_state.last_attempt_at)
        ),
        Some(false) => format!(
            "Last update: {} (failed)",
            format_refresh_timestamp(refresh_state.last_attempt_at)
        ),
        None => "Last update: never".to_string(),
    }
}

fn refresh_log_text(refresh_state: &RefreshState) -> String {
    let current_status = match refresh_state.last_update_successful {
        Some(true) => "successful",
        Some(false) => "failed",
        None => "not run yet",
    };

    let latest_error = refresh_state.last_error.as_deref().unwrap_or("none");

    let mut lines = vec![
        format!(
            "Last attempted: {}",
            format_refresh_timestamp(refresh_state.last_attempt_at)
        ),
        format!(
            "Last successful: {}",
            format_refresh_timestamp(refresh_state.last_success_at)
        ),
        format!("Current status: {}", current_status),
        format!("Latest error: {}", latest_error),
        String::new(),
        "Recent refresh log:".to_string(),
    ];

    if refresh_state.log_entries.is_empty() {
        lines.push("No refresh attempts recorded yet.".to_string());
    } else {
        for entry in refresh_state.log_entries.iter().rev() {
            let status = if entry.successful {
                "success"
            } else {
                "failure"
            };
            lines.push(format!(
                "{} | {} | {}",
                entry.timestamp.format("%Y-%m-%d %H:%M:%S"),
                status,
                entry.message
            ));
        }
    }

    lines.join("\n")
}

fn show_refresh_log_dialog(parent: Option<&gtk::Window>, refresh_state: &RefreshState) {
    let dialog = gtk::Dialog::new();
    dialog.set_title("Calendar Refresh Log");
    dialog.set_modal(true);
    if let Some(parent) = parent {
        dialog.set_transient_for(Some(parent));
    }
    dialog.add_button("Close", gtk::ResponseType::Close);
    dialog.set_default_size(760, 420);

    let content_area = dialog.content_area();
    let scrolled_window =
        gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scrolled_window.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scrolled_window.set_hexpand(true);
    scrolled_window.set_vexpand(true);

    let text_view = gtk::TextView::new();
    text_view.set_editable(false);
    text_view.set_cursor_visible(false);
    text_view.set_monospace(true);
    text_view.set_wrap_mode(gtk::WrapMode::WordChar);
    text_view
        .buffer()
        .expect("TextView buffer must exist")
        .set_text(&refresh_log_text(refresh_state));

    scrolled_window.add(&text_view);
    content_area.pack_start(&scrolled_window, true, true, 0);

    dialog.connect_response(|dialog, _| {
        dialog.close();
    });
    dialog.show_all();
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
        let palette = if now >= event.start_timestamp && now <= event.end_timestamp {
            EventPalette {
                background: "rgba(245, 184, 82, 0.92)",
                border: "#c17a16",
                text: "#2c2418",
            }
        } else if now < event.start_timestamp {
            EventPalette {
                background: "rgba(204, 217, 246, 0.90)",
                border: "#7f98c9",
                text: "#22304d",
            }
        } else {
            EventPalette {
                background: "rgba(226, 229, 232, 0.78)",
                border: "#c1c8cf",
                text: "#59636f",
            }
        };

        load_css(
            &button.style_context(),
            &format!(
                "button {{ \
                    background: {}; \
                    border: 1px solid {}; \
                    border-radius: 5px; \
                    box-shadow: inset 0 1px rgba(255, 255, 255, 0.34); \
                    color: {}; \
                    text-shadow: none; \
                }} \
                button:hover {{ border-color: {}; }}",
                palette.background, palette.border, palette.text, palette.text
            ),
        );

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
        style_label(&label, palette.text);
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
        let all_day_container = gtk::Box::new(gtk::Orientation::Vertical, 4);
        all_day_container.set_margin_bottom(if all_day_events.is_empty() { 6 } else { 12 });

        // Add "All Day" label
        let all_day_label = gtk::Label::new(Some("All Day"));
        all_day_label.set_xalign(0.0);
        all_day_label.set_margin_bottom(2);
        all_day_label.set_markup("All Day");
        style_label_with_css(&all_day_label, TEXT_SUBTLE, "font-size: 13px;");
        all_day_container.pack_start(&all_day_label, false, false, 0);

        // Create horizontal box for all-day events
        let all_day_events_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);

        // Set a minimum height for the all-day events box to ensure consistent spacing
        all_day_events_box.set_size_request(-1, if all_day_events.is_empty() { 12 } else { 40 });

        if !all_day_events.is_empty() {
            let button_width = ((TIMELINE_MIN_WIDTH - (6 * (all_day_events.len() as i32 + 1)))
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
        meeting_area.set_size_request(TIMELINE_MIN_WIDTH, (end_hour - start_hour) * HOUR_HEIGHT);

        // Add background with color transitions at working hours
        let background_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        background_box.set_size_request(TIMELINE_MIN_WIDTH, (end_hour - start_hour) * HOUR_HEIGHT);
        let css = format!(
            "box {{ \
                background-color: {}; \
                margin: 0; \
                padding: 0; \
            }}",
            TIMELINE_BACKGROUND
        );
        load_css(&background_box.style_context(), &css);
        meeting_area.put(&background_box, 0, 0);
        let timeline_rail = gtk::Box::new(gtk::Orientation::Vertical, 0);
        timeline_rail.set_size_request(2, (end_hour - start_hour) * HOUR_HEIGHT);
        load_css(
            &timeline_rail.style_context(),
            &format!(
                "box {{ background-color: {}; margin: 0; padding: 0; }}",
                TIMELINE_RAIL
            ),
        );
        meeting_area.put(&timeline_rail, 0, 0);

        // Add hour markers and grid lines
        for hour in start_hour..=end_hour {
            let y_position = (hour - start_hour) * HOUR_HEIGHT;

            // Hour label
            let label = gtk::Label::new(Some(&format!("{:02}:00", hour)));
            label.set_xalign(1.0);
            label.set_margin_end(5);
            style_label_with_css(&label, TEXT_SUBTLE, "font-size: 13px;");
            time_column.put(&label, 0, y_position);

            // Hour separator with styling
            let separator = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            separator.set_size_request(TIMELINE_MIN_WIDTH, -1);

            // Different styles for start/end of day vs regular hours
            let css = if hour == start_hour || hour == end_hour {
                format!(
                    "box {{ background-color: {}; min-height: 2px; margin: 0; padding: 0; }}",
                    TIMELINE_GRID_STRONG
                )
            } else {
                format!(
                    "box {{ background-color: {}; min-height: 1px; margin: 0; padding: 0; }}",
                    TIMELINE_GRID
                )
            };

            load_css(&separator.style_context(), &css);

            meeting_area.put(&separator, 0, y_position);
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
            let button_width =
                ((TIMELINE_MIN_WIDTH - (spacing * (group_size + 1))) / group_size).max(200);

            for (index, event) in group.iter().enumerate() {
                let event_start = event.start_timestamp.with_timezone(&Local);
                let event_end = event.end_timestamp.with_timezone(&Local);

                // Calculate position
                let start_minutes =
                    (event_start.hour() as i32 - start_hour) * 60 + event_start.minute() as i32;
                let duration_minutes =
                    event_end.signed_duration_since(event_start).num_minutes() as i32;

                let touches_previous_event = regular_events
                    .iter()
                    .any(|other| other.end_timestamp == event.start_timestamp);
                let y_position =
                    (start_minutes * HOUR_HEIGHT) / 60 - if touches_previous_event { 1 } else { 0 };
                let height = ((duration_minutes * HOUR_HEIGHT) / 60
                    + if touches_previous_event { 1 } else { 0 })
                .max(30);
                let x_position = spacing + (button_width + spacing) * index as i32;

                let button = Self::create_event_button(event, button_width, height, true);
                meeting_area.put(&button, x_position, y_position);
            }
        }

        // Current time indicator is added last so it stays visible over event buttons.
        if is_today {
            let now = Local::now();
            let current_hour = now.hour() as i32;
            let current_minute = now.minute() as i32;
            if current_hour >= start_hour && current_hour <= end_hour {
                let minutes_from_start = (current_hour - start_hour) * 60 + current_minute;
                let y_position = (minutes_from_start * HOUR_HEIGHT) / 60;

                let current_time_marker = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                current_time_marker.set_size_request(TIMELINE_MIN_WIDTH, -1);
                load_css(
                    &current_time_marker.style_context(),
                    &format!(
                        "box {{ background-color: {}; min-height: 2px; margin: 0; padding: 0; }}",
                        CURRENT_TIME_MARKER
                    ),
                );

                meeting_area.put(&current_time_marker, 0, y_position);

                let current_time_cap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                current_time_cap.set_size_request(8, 8);
                load_css(
                    &current_time_cap.style_context(),
                    &format!(
                        "box {{ background-color: {}; border-radius: 4px; margin: 0; padding: 0; }}",
                        CURRENT_TIME_MARKER
                    ),
                );
                meeting_area.put(&current_time_cap, 0, y_position - 3);
            }
        }

        let total_height = (end_hour - start_hour) * HOUR_HEIGHT;

        // Assemble the layout
        layout_box.pack_start(&time_column, false, false, 0);
        layout_box.pack_start(&meeting_area, true, true, 0);

        // Set a minimum height for the layout - no need for +1 as we want to end exactly at end_hour
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
    refresh_state: Arc<Mutex<RefreshState>>,
    start_hour: i32,
    end_hour: i32,
    future_days: i32,
}

impl WindowManager {
    pub fn new(
        start_hour: i32,
        end_hour: i32,
        future_days: i32,
        refresh_state: Arc<Mutex<RefreshState>>,
    ) -> Self {
        WindowManager {
            current_window: None,
            day_events: Arc::new(Mutex::new(Vec::new())),
            refresh_state,
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

    fn day_label_text(day_index: usize) -> String {
        if day_index == 0 {
            "Today".to_string()
        } else if day_index == 1 {
            "Tomorrow".to_string()
        } else {
            let date = Local::now().date_naive() + chrono::Duration::days(day_index as i64);
            format!("{}", date.format("%A, %B %d"))
        }
    }

    fn build_day_box(&self, day_index: usize, events: &[Event]) -> gtk::Box {
        let day_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let label_text = Self::day_label_text(day_index);

        let day_label = gtk::Label::new(Some(&label_text));
        day_label.set_xalign(0.0);
        day_label.set_margin_bottom(4);
        day_label.set_markup(&format!("<b>{}</b>", label_text));
        style_label_with_css(&day_label, TEXT_PRIMARY, "font-size: 15px;");

        day_box.pack_start(&day_label, false, false, 0);

        let timeline = TimelineView::new(
            events.to_vec(),
            self.start_hour,
            self.end_hour,
            day_index == 0,
        );
        day_box.pack_start(&timeline.container, true, true, 0);

        day_box
    }

    fn build_days_view(&self, day_events: &[Vec<Event>]) -> gtk::ScrolledWindow {
        let scrolled_window =
            gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scrolled_window.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);

        let days_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        for (day_index, events) in day_events.iter().enumerate() {
            let day_box = self.build_day_box(day_index, events);
            days_box.pack_start(&day_box, true, true, 0);
        }

        scrolled_window.add(&days_box);
        scrolled_window
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
            DAY_MIN_WIDTH * (self.future_days + 1),
            calculate_window_height(self.start_hour, self.end_hour),
        );

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        main_box.set_margin_start(6);
        main_box.set_margin_end(6);
        main_box.set_margin_top(6);
        main_box.set_margin_bottom(6);

        let scrolled_window = self.build_days_view(&day_events);
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

                let scrolled_window = self.build_days_view(&events);
                main_box.pack_start(&scrolled_window, true, true, 0);
                main_box.show_all();
            }
        }
    }

    pub fn today_events(&self) -> Vec<Event> {
        self.day_events
            .lock()
            .unwrap()
            .first()
            .cloned()
            .unwrap_or_default()
    }

    pub fn refresh_state_snapshot(&self) -> RefreshState {
        self.refresh_state.lock().unwrap().clone()
    }

    pub fn refresh_log_dialog_data(&self) -> (Option<gtk::Window>, RefreshState) {
        (self.current_window.clone(), self.refresh_state_snapshot())
    }
}

pub fn create_indicator_menu(
    today_events: &[Event],
    indicator: &mut AppIndicator,
    window_manager: Arc<Mutex<WindowManager>>,
) {
    let mut m: Menu = gtk::Menu::new();
    let mut nof_upcoming_meetings = 0;
    let refresh_state = {
        let wm = window_manager.lock().unwrap();
        wm.refresh_state_snapshot()
    };

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

    let refresh_status_item = gtk::MenuItem::with_label(&refresh_status_menu_label(&refresh_state));
    let log_window_manager = Arc::clone(&window_manager);
    refresh_status_item.connect_activate(move |_| {
        let (parent, refresh_state) = {
            let wm = log_window_manager.lock().unwrap();
            wm.refresh_log_dialog_data()
        };
        show_refresh_log_dialog(parent.as_ref(), &refresh_state);
    });
    m.append(&gtk::SeparatorMenuItem::new());
    m.append(&refresh_status_item);

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
    if refresh_state.last_update_successful == Some(false) {
        log::warn!("calendar refresh failed");
        set_error_icon(indicator);
    } else if nof_upcoming_meetings > 0 {
        log::debug!("some meetings upcoming");
        set_some_meetings_left_icon(indicator);
    } else {
        log::debug!("no meetings upcoming");
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
    } else if let Err(e) = notification.show() {
        log::warn!("could not show notification: {}", e);
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
    refresh_state: Arc<Mutex<RefreshState>>,
) -> (
    AppIndicator,
    Arc<Mutex<WindowManager>>,
    Receiver<(String, ())>,
) {
    // Initialize GTK
    gtk::init().unwrap();

    // Create window manager
    let window_manager = Arc::new(Mutex::new(WindowManager::new(
        start_hour,
        end_hour,
        future_days,
        refresh_state,
    )));

    // Set up D-Bus connection
    log::info!("starting D-Bus integration");
    let connection = Connection::new_session().expect("Failed to connect to D-Bus");
    connection
        .request_name("net.aggregat4.Meeters", false, true, false)
        .expect("Failed to request D-Bus name");
    log::debug!("D-Bus name net.aggregat4.Meeters acquired");

    // Create a channel for D-Bus requests using async-channel
    let (dbus_sender, dbus_receiver) = async_channel::bounded(10);

    // Create D-Bus interface
    let mut cr = Crossroads::new();

    let iface_token = {
        let show_sender = dbus_sender.clone();
        let close_sender = dbus_sender.clone();
        let toggle_sender = dbus_sender.clone();

        cr.register("net.aggregat4.Meeters", move |b| {
            let show_sender = show_sender.clone();
            b.method("ShowWindow", (), (), move |_, _, ()| {
                if let Err(e) = show_sender.send_blocking(("show".to_string(), ())) {
                    log::error!("could not dispatch D-Bus show action to GUI thread: {}", e);
                }
                Ok(())
            });

            let close_sender = close_sender.clone();
            b.method("CloseWindow", (), (), move |_, _, ()| {
                if let Err(e) = close_sender.send_blocking(("close".to_string(), ())) {
                    log::error!("could not dispatch D-Bus close action to GUI thread: {}", e);
                }
                Ok(())
            });

            let toggle_sender = toggle_sender.clone();
            b.method("ToggleWindow", (), (), move |_, _, ()| {
                if let Err(e) = toggle_sender.send_blocking(("toggle".to_string(), ())) {
                    log::error!(
                        "could not dispatch D-Bus toggle action to GUI thread: {}",
                        e
                    );
                }
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
