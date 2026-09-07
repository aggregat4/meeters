use crate::config::get_config_directory;
use crate::domain::{Event, RefreshState, ResponseStatus, ONLINE_MEETING_MARKER};
use crate::gui::actions::open_meeting;
use crate::gui::refresh_log::refresh_status_menu_label;
use async_channel::{Receiver, Sender};
use chrono::prelude::*;
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use notify_rust::{Notification, Timeout};
use std::path::PathBuf;
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    OpenMeeting(String),
    ShowWindow,
    ShowRefreshLog,
    Quit,
}

pub struct MeetingTray {
    events: Vec<Event>,
    refresh_status: String,
    icon_name: String,
    icon_theme_path: String,
    sender: Sender<TrayAction>,
}

impl MeetingTray {
    fn dispatch(&self, action: TrayAction) {
        if let Err(error) = self.sender.try_send(action) {
            log::warn!("could not dispatch tray action: {}", error);
        }
    }
}

impl ksni::Tray for MeetingTray {
    const MENU_ON_ACTIVATE: bool = true;
    fn id(&self) -> String {
        "meeters".into()
    }
    fn title(&self) -> String {
        "Meeters".into()
    }
    fn icon_name(&self) -> String {
        self.icon_name.clone()
    }
    fn icon_theme_path(&self) -> String {
        self.icon_theme_path.clone()
    }
    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        log::warn!(
            "desktop tray unavailable: {:?}; waiting for a tray host",
            reason
        );
        true
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let mut items = Vec::new();
        if self.events.is_empty() {
            items.push(
                StandardItem {
                    label: "No Events Today".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }
        for event in &self.events {
            let url = event.meeturl.clone();
            items.push(
                StandardItem {
                    label: event_menu_label(event, Local::now()),
                    enabled: url.is_some(),
                    activate: Box::new(move |tray: &mut Self| {
                        if let Some(url) = &url {
                            tray.dispatch(TrayAction::OpenMeeting(url.clone()));
                        }
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }
        items.push(ksni::MenuItem::Separator);
        items.push(
            StandardItem {
                label: self.refresh_status.clone(),
                activate: Box::new(|tray: &mut Self| tray.dispatch(TrayAction::ShowRefreshLog)),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Show Meetings Window".into(),
                activate: Box::new(|tray: &mut Self| tray.dispatch(TrayAction::ShowWindow)),
                ..Default::default()
            }
            .into(),
        );
        items.push(ksni::MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| tray.dispatch(TrayAction::Quit)),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}

fn event_menu_label(event: &Event, now: DateTime<Local>) -> String {
    let time = if event.all_day {
        "All Day".to_string()
    } else {
        format!(
            "{} - {}",
            event.start_timestamp.format("%H:%M"),
            event.end_timestamp.format("%H:%M")
        )
    };
    let prefix = if event.all_day {
        ""
    } else if now < event.start_timestamp {
        "◦ "
    } else if now <= event.end_timestamp {
        "• "
    } else {
        "✓ "
    };
    let marker = if event.meeturl.is_some() {
        ONLINE_MEETING_MARKER
    } else {
        ""
    };
    // DBusMenu uses underscores as mnemonic markers. Escape literal underscores.
    format!("{}{}: {}{}", prefix, time, event.summary, marker).replace('_', "__")
}

fn find_icon_directory() -> Option<PathBuf> {
    let mut directories = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            directories.push(parent.to_path_buf());
        }
    }
    directories.push(get_config_directory());
    directories
        .into_iter()
        .find(|dir| dir.join("meeters-appindicator.png").is_file())
}

pub struct Indicator {
    handle: ksni::blocking::Handle<MeetingTray>,
    icon_directory: Option<PathBuf>,
}

impl Drop for Indicator {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

pub fn create_indicator() -> Result<(Indicator, Receiver<TrayAction>), ksni::Error> {
    let (sender, receiver) = async_channel::unbounded();
    let icon_directory = find_icon_directory();
    let tray = MeetingTray {
        events: Vec::new(),
        refresh_status: "Calendar not refreshed yet".into(),
        icon_name: if icon_directory.is_some() {
            "meeters-appindicator"
        } else {
            "x-office-calendar"
        }
        .into(),
        icon_theme_path: icon_directory
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        sender,
    };
    // Keep serving if the desktop tray host is temporarily absent or restarts.
    let handle = tray.assume_sni_available(true).spawn()?;
    Ok((
        Indicator {
            handle,
            icon_directory,
        },
        receiver,
    ))
}

pub fn create_indicator_menu(events: &[Event], indicator: &Indicator, state: &RefreshState) {
    let now = Local::now();
    let icon = if state.last_update_successful == Some(false) {
        "meeters-appindicator-error"
    } else if events
        .iter()
        .any(|event| !event.all_day && event.end_timestamp >= now)
    {
        "meeters-appindicator-somemeetingsleft"
    } else {
        "meeters-appindicator-nomeetingsleft"
    };
    let icon = match &indicator.icon_directory {
        Some(dir) if dir.join(format!("{}.png", icon)).is_file() => icon,
        Some(_) => "meeters-appindicator",
        None => "x-office-calendar",
    };
    indicator.handle.update(|tray| {
        tray.events = events.to_vec();
        tray.refresh_status = refresh_status_menu_label(state);
        tray.icon_name = icon.into();
    });
}

pub fn show_event_notification(event: Event) {
    if event.meeturl.is_some() {
        thread::spawn(move || show_event_notification_now(event));
    } else {
        show_event_notification_now(event);
    }
}

fn show_event_notification_now(event: Event) {
    let summary_str = &format!(
        "{} - {}",
        event.start_timestamp.format("%H:%M"),
        event.summary
    );
    let notification_body = notification_body(&event);
    let mut notification = Notification::new();
    notification
        .summary(summary_str)
        .body(&notification_body)
        .icon("appointment-new")
        .urgency(notify_rust::Urgency::Critical)
        .timeout(Timeout::Never);

    if let Some(meeturl) = event.meeturl {
        let handle = notification
            .action(
                &format!("{}{}", MEETERS_NOTIFICATION_ACTION_OPEN_MEETING, meeturl),
                "Open Zoom Meeting",
            )
            .show();
        match handle {
            Ok(handle) => handle.wait_for_action(|action| {
                if let Some(meeting) = action.strip_prefix(MEETERS_NOTIFICATION_ACTION_OPEN_MEETING)
                {
                    let meeting = meeting.to_string();
                    glib::idle_add_once(move || open_meeting(&meeting));
                }
            }),
            Err(e) => log::warn!("could not show notification: {}", e),
        }
    } else if let Err(e) = notification.show() {
        log::warn!("could not show notification: {}", e);
    }
}

fn notification_body(event: &Event) -> String {
    let mut lines = Vec::new();
    lines.push(
        event
            .meeturl
            .clone()
            .unwrap_or_else(|| "No Zoom Meeting".to_string()),
    );

    let declined_rooms = event
        .metadata
        .rooms
        .iter()
        .filter(|room| room.response == Some(ResponseStatus::Declined))
        .map(|room| format!("{}: DECLINED", room.name))
        .collect::<Vec<_>>();

    if !declined_rooms.is_empty() {
        let label = if declined_rooms.len() == 1 {
            "Room"
        } else {
            "Rooms"
        };
        lines.push(format!("{}: {}", label, declined_rooms.join(", ")));
    }

    lines.join("\n")
}

const MEETERS_NOTIFICATION_ACTION_OPEN_MEETING: &str = "meeters_open_meeting:";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{EventMetadata, Participant};

    fn event_with_rooms(rooms: Vec<Participant>) -> Event {
        Event {
            summary: "Planning".to_string(),
            description: String::new(),
            location: String::new(),
            meeturl: Some("https://example.com/meeting".to_string()),
            metadata: EventMetadata {
                organizer: None,
                rooms,
                required_attendees: Vec::new(),
                optional_attendees: Vec::new(),
            },
            all_day: false,
            start_timestamp: chrono_tz::Europe::Berlin
                .with_ymd_and_hms(2026, 5, 19, 9, 0, 0)
                .unwrap(),
            end_timestamp: chrono_tz::Europe::Berlin
                .with_ymd_and_hms(2026, 5, 19, 9, 30, 0)
                .unwrap(),
        }
    }

    #[test]
    fn notification_body_includes_declined_room() {
        let event = event_with_rooms(vec![Participant {
            name: "Room 3A".to_string(),
            response: Some(ResponseStatus::Declined),
        }]);

        assert_eq!(
            notification_body(&event),
            "https://example.com/meeting\nRoom: Room 3A: DECLINED"
        );
    }

    #[test]
    fn notification_body_omits_accepted_room() {
        let event = event_with_rooms(vec![Participant {
            name: "Room 3A".to_string(),
            response: Some(ResponseStatus::Accepted),
        }]);

        assert_eq!(notification_body(&event), "https://example.com/meeting");
    }
}
