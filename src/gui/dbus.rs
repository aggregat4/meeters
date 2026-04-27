use async_channel::Receiver;
use dbus::blocking::Connection;
use dbus_crossroads::Crossroads;
use std::thread;

pub fn start_dbus_service() -> Receiver<(String, ())> {
    log::info!("starting D-Bus integration");
    let connection = Connection::new_session().expect("Failed to connect to D-Bus");
    connection
        .request_name("net.aggregat4.Meeters", false, true, false)
        .expect("Failed to request D-Bus name");
    log::debug!("D-Bus name net.aggregat4.Meeters acquired");

    let (dbus_sender, dbus_receiver) = async_channel::bounded(10);
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

    thread::spawn(move || {
        cr.serve(&connection).unwrap();
    });

    dbus_receiver
}
