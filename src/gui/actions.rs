pub fn open_meeting(meet_url: &str) {
    if let Err(error) =
        gtk::gio::AppInfo::launch_default_for_uri(meet_url, None::<&gtk::gio::AppLaunchContext>)
    {
        log::error!("error trying to open the meeting URL: {}", error);
    }
}
