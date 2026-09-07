use crate::domain::RefreshState;
use crate::gui::actions::open_meeting;
use crate::gui::dbus::start_dbus_service;
use crate::gui::indicator::{create_indicator, create_indicator_menu, Indicator, TrayAction};
use crate::gui::refresh_log::show_refresh_log_dialog;
use crate::gui::window::WindowManager;
use async_channel::Receiver;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub fn initialize_gui(
    application: &gtk::Application,
    start_hour: i32,
    end_hour: i32,
    future_days: i32,
    refresh_state: Arc<Mutex<RefreshState>>,
) -> Result<
    (
        Indicator,
        Rc<RefCell<WindowManager>>,
        Receiver<(String, ())>,
    ),
    ksni::Error,
> {
    let window_manager = Rc::new(RefCell::new(WindowManager::new(
        application,
        start_hour,
        end_hour,
        future_days,
        refresh_state,
    )));
    let dbus_receiver = start_dbus_service();
    let (indicator, tray_receiver) = create_indicator()?;
    create_indicator_menu(
        &[],
        &indicator,
        &window_manager.borrow().refresh_state_snapshot(),
    );

    // The first activation starts in the tray. Launching the application again
    // presents the existing window rather than starting another polling worker.
    let first_activation = Cell::new(true);
    let activation_window_manager = Rc::downgrade(&window_manager);
    application.connect_activate(move |_| {
        if !first_activation.replace(false) {
            if let Some(wm) = activation_window_manager.upgrade() {
                wm.borrow_mut().show_window();
            }
        }
    });

    let quit = gtk::gio::SimpleAction::new("quit", None);
    let weak_application = application.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(application) = weak_application.upgrade() {
            application.quit();
        }
    });
    application.add_action(&quit);
    application.set_accels_for_action("app.quit", &["<Primary>q"]);

    let tray_window_manager = Rc::clone(&window_manager);
    let application = application.clone();
    glib::MainContext::default().spawn_local(async move {
        while let Ok(action) = tray_receiver.recv().await {
            match action {
                TrayAction::OpenMeeting(url) => open_meeting(&url),
                TrayAction::ShowWindow => tray_window_manager.borrow_mut().show_window(),
                TrayAction::ShowRefreshLog => {
                    let (parent, state) = tray_window_manager.borrow().refresh_log_dialog_data();
                    show_refresh_log_dialog(&application, parent.as_ref(), &state);
                }
                TrayAction::Quit => application.quit(),
            }
        }
    });

    Ok((indicator, window_manager, dbus_receiver))
}

pub fn run_gui_main_loop(application: &gtk::Application) {
    // A tray application must survive even when it has no open windows.
    let _hold = application.hold();
    application.run();
}
