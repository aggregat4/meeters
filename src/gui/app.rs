use crate::domain::RefreshState;
use crate::gui::dbus::start_dbus_service;
use crate::gui::indicator::{create_indicator, create_indicator_menu};
use crate::gui::window::WindowManager;
use async_channel::Receiver;
use libappindicator::AppIndicator;
use std::sync::{Arc, Mutex};

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
    gtk::init().unwrap();

    let window_manager = Arc::new(Mutex::new(WindowManager::new(
        start_hour,
        end_hour,
        future_days,
        refresh_state,
    )));
    let dbus_receiver = start_dbus_service();

    let mut indicator = create_indicator();
    create_indicator_menu(&[], &mut indicator, Arc::clone(&window_manager));

    (indicator, window_manager, dbus_receiver)
}

pub fn run_gui_main_loop() {
    gtk::main();
}
