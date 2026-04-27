use crate::domain::RefreshState;
use chrono::prelude::*;
use gtk::prelude::*;

fn format_refresh_timestamp(timestamp: Option<DateTime<Local>>) -> String {
    timestamp
        .map(|ts| ts.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "never".to_string())
}

pub fn refresh_status_menu_label(refresh_state: &RefreshState) -> String {
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

pub fn show_refresh_log_dialog(parent: Option<&gtk::Window>, refresh_state: &RefreshState) {
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
