use crate::domain::CalendarError;

const EWS_PASSWORD_SERVICE: &str = "net.aggregat4.meeters.exchange";

fn calendar_error(msg: impl Into<String>) -> CalendarError {
    CalendarError { msg: msg.into() }
}

pub enum StoredPassword {
    Found(String),
    Missing,
}

pub fn get_ews_password(user: &str) -> Result<StoredPassword, CalendarError> {
    let entry = keyring::Entry::new(EWS_PASSWORD_SERVICE, user)
        .map_err(|e| calendar_error(format!("Could not open desktop keyring: {}", e)))?;
    match entry.get_password() {
        Ok(password) => Ok(StoredPassword::Found(password)),
        Err(keyring::Error::NoEntry) => Ok(StoredPassword::Missing),
        Err(e) => Err(calendar_error(format!(
            "Could not read EWS password from desktop keyring: {}",
            e
        ))),
    }
}

pub fn store_ews_password(user: &str, password: &str) -> Result<(), CalendarError> {
    let entry = keyring::Entry::new(EWS_PASSWORD_SERVICE, user)
        .map_err(|e| calendar_error(format!("Could not open desktop keyring: {}", e)))?;
    entry.set_password(password).map_err(|e| {
        calendar_error(format!(
            "Could not store EWS password in desktop keyring: {}",
            e
        ))
    })
}
