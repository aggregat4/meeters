use gtk::prelude::*;

pub fn show_ews_password_dialog(
    parent: Option<&gtk::Window>,
    endpoint: &str,
    user: &str,
    replacing_existing_password: bool,
) -> Option<String> {
    let title = if replacing_existing_password {
        "Update Exchange Password"
    } else {
        "Exchange Password Required"
    };
    let message = if replacing_existing_password {
        format!(
            "The stored Exchange password for {} was rejected. Enter a new password for {}.",
            user, endpoint
        )
    } else {
        format!(
            "Enter the Exchange password for {} at {}. It will be stored in your desktop keyring.",
            user, endpoint
        )
    };

    let dialog = gtk::Dialog::with_buttons(
        Some(title),
        parent,
        gtk::DialogFlags::MODAL,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Save", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_default_response(gtk::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);

    let label = gtk::Label::new(Some(&message));
    label.set_line_wrap(true);
    label.set_xalign(0.0);
    content.pack_start(&label, false, false, 0);

    let password_entry = gtk::Entry::new();
    password_entry.set_visibility(false);
    password_entry.set_activates_default(true);
    password_entry.set_input_purpose(gtk::InputPurpose::Password);
    content.pack_start(&password_entry, false, false, 0);

    dialog.show_all();
    let response = dialog.run();
    let password = if response == gtk::ResponseType::Accept {
        let password = password_entry.text().to_string();
        if password.is_empty() {
            None
        } else {
            Some(password)
        }
    } else {
        None
    };
    dialog.close();

    password
}
