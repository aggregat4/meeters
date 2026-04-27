pub fn open_meeting(meet_url: &str) {
    match gtk::show_uri_on_window(None::<&gtk::Window>, meet_url, gtk::current_event_time()) {
        Ok(_) => (),
        Err(e) => log::error!("error trying to open the meeting URL: {}", e),
    }
}
