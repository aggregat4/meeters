mod actions;
mod app;
mod dbus;
mod indicator;
mod password_prompt;
mod refresh_log;
mod styles;
mod timeline;
mod window;

pub use app::{initialize_gui, run_gui_main_loop};
pub use indicator::{create_indicator_menu, show_event_notification};
pub use password_prompt::show_ews_password_dialog;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::RefreshState;
    use gtk::prelude::*;
    use std::sync::{Arc, Mutex};

    fn find_entry(widget: &gtk::Widget) -> Option<gtk::Entry> {
        if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
            return Some(entry.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(entry) = find_entry(&widget) {
                return Some(entry);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn answer_password_prompt(response: gtk::ResponseType, text: &'static str) {
        glib::idle_add_local_once(move || {
            let dialog = gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Dialog>().ok())
                .find(|dialog| dialog.title().as_deref() == Some("Exchange Password Required"))
                .expect("password dialog must be presented before awaiting a response");
            find_entry(dialog.upcast_ref())
                .expect("password entry")
                .set_text(text);
            dialog.response(response);
        });
    }

    #[test]
    #[ignore = "requires an isolated D-Bus session and display; see README"]
    fn gtk4_window_and_password_dialogs() {
        let app = gtk::Application::builder()
            .application_id("net.aggregat4.Meeters.GuiTest")
            .build();
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let state = Arc::new(Mutex::new(RefreshState::new(10, "ICS")));
        let mut manager = window::WindowManager::new(&app, 8, 20, 1, state);
        manager.update_events(vec![Vec::new(), Vec::new()]);
        manager.show_window();
        let window = manager.current_window.clone().unwrap();
        assert!(window.is_visible());
        window.close();
        assert!(!window.is_visible());
        manager.show_window();
        assert_eq!(manager.current_window.as_ref(), Some(&window));
        assert!(window.is_visible());
        manager.update_events(vec![Vec::new()]);
        assert!(window.child().is_some());

        let context = glib::MainContext::default();
        for (response, text, expected) in [
            (
                gtk::ResponseType::Accept,
                "test-password",
                Some("test-password"),
            ),
            (gtk::ResponseType::Accept, "", None),
            (gtk::ResponseType::Cancel, "discard-this", None),
            (gtk::ResponseType::DeleteEvent, "discard-this", None),
        ] {
            answer_password_prompt(response, text);
            let password = context.block_on(password_prompt::show_ews_password_dialog(
                &app,
                if response == gtk::ResponseType::Cancel {
                    None
                } else {
                    Some(&window)
                },
                "https://exchange.example.invalid/EWS/Exchange.asmx",
                "test@example.invalid",
                false,
            ));
            assert_eq!(password.as_deref(), expected);
        }
        window.destroy();
    }
}
