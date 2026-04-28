use crate::config::get_config_directory;
use crate::domain::{Event, RefreshState, ONLINE_MEETING_MARKER};
use crate::gui::actions::open_meeting;
use crate::gui::refresh_log::{refresh_status_menu_label, show_refresh_log_dialog};
use crate::gui::window::WindowManager;
use chrono::prelude::*;
use gtk::prelude::*;
use gtk::Menu;
use libappindicator::{AppIndicator, AppIndicatorStatus};
use notify_rust::Notification;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

fn set_some_meetings_left_icon(indicator: &mut AppIndicator) {
    if let Some(icon_path) = find_icon_path() {
        indicator.set_icon(
            get_icon_path_with_fallback(
                icon_path,
                "meeters-appindicator-somemeetingsleft.png".to_string(),
            )
            .to_str()
            .unwrap(),
        );
    }
}

fn set_no_meetings_left_icon(indicator: &mut AppIndicator) {
    if let Some(icon_path) = find_icon_path() {
        indicator.set_icon(
            get_icon_path_with_fallback(
                icon_path,
                "meeters-appindicator-nomeetingsleft.png".to_string(),
            )
            .to_str()
            .unwrap(),
        );
    }
}

fn get_icon_path_with_fallback(icon_path: PathBuf, icon_filename: String) -> PathBuf {
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
                Some(_) => ONLINE_MEETING_MARKER,
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
    set_icon_for_refresh_state(&refresh_state, nof_upcoming_meetings, indicator);
    indicator.set_menu(&mut m);
}

fn set_icon_for_refresh_state(
    refresh_state: &RefreshState,
    nof_upcoming_meetings: i32,
    indicator: &mut AppIndicator,
) {
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

const MEETERS_NOTIFICATION_ACTION_OPEN_MEETING: &str = "meeters_open_meeting:";
