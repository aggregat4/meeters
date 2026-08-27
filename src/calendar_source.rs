use std::sync::mpsc;
use std::time::Duration;

use chrono::DateTime;
use chrono_tz::Tz;
use ureq::Agent;

use crate::config::CalendarSourceConfig;
use crate::domain::{CalendarError, Event};
use crate::ews::{self, EwsConfig, EwsError};
use crate::meeters_ical;
use crate::secrets::{self, StoredPassword};

#[derive(Debug)]
pub struct EwsPasswordPrompt {
    pub endpoint: String,
    pub user: String,
    pub replacing_existing_password: bool,
    pub response_sender: mpsc::Sender<Option<String>>,
}

#[derive(Clone)]
pub struct CalendarSource {
    config: CalendarSourceConfig,
    password_prompt_sender: async_channel::Sender<EwsPasswordPrompt>,
}

impl CalendarSource {
    pub fn new(
        config: CalendarSourceConfig,
        password_prompt_sender: async_channel::Sender<EwsPasswordPrompt>,
    ) -> Self {
        CalendarSource {
            config,
            password_prompt_sender,
        }
    }

    pub fn fetch_events(
        &self,
        local_tz: &Tz,
        use_zoommtg: bool,
        start_time: DateTime<Tz>,
        end_time: DateTime<Tz>,
    ) -> Result<Vec<Event>, CalendarError> {
        match &self.config {
            CalendarSourceConfig::Ics { url, user_agent } => get_ical(url, user_agent)
                .and_then(|text| meeters_ical::extract_events(&text, local_tz, use_zoommtg)),
            CalendarSourceConfig::Ews { url, user } => self.fetch_ews_events(
                &EwsConfig {
                    url: url.clone(),
                    user: user.clone(),
                },
                local_tz,
                use_zoommtg,
                start_time,
                end_time,
            ),
        }
    }

    fn fetch_ews_events(
        &self,
        ews_config: &EwsConfig,
        local_tz: &Tz,
        use_zoommtg: bool,
        start_time: DateTime<Tz>,
        end_time: DateTime<Tz>,
    ) -> Result<Vec<Event>, CalendarError> {
        let password = self.get_or_prompt_password(ews_config, false)?;
        match ews::fetch_events(
            ews_config,
            &password,
            local_tz,
            start_time,
            end_time,
            use_zoommtg,
        ) {
            Ok(events) => Ok(events),
            Err(EwsError::AuthFailed) => {
                let password = self.get_or_prompt_password(ews_config, true)?;
                ews::fetch_events(
                    ews_config,
                    &password,
                    local_tz,
                    start_time,
                    end_time,
                    use_zoommtg,
                )
                .map_err(map_ews_error)
            }
            Err(error) => Err(map_ews_error(error)),
        }
    }

    fn get_or_prompt_password(
        &self,
        ews_config: &EwsConfig,
        replacing_existing_password: bool,
    ) -> Result<String, CalendarError> {
        if !replacing_existing_password {
            match secrets::get_ews_password(&ews_config.user)? {
                StoredPassword::Found(password) => return Ok(password),
                StoredPassword::Missing => {}
            }
        }

        let password = self.prompt_for_password(ews_config, replacing_existing_password)?;
        secrets::store_ews_password(&ews_config.user, &password)?;
        Ok(password)
    }

    fn prompt_for_password(
        &self,
        ews_config: &EwsConfig,
        replacing_existing_password: bool,
    ) -> Result<String, CalendarError> {
        let (response_sender, response_receiver) = mpsc::channel();
        self.password_prompt_sender
            .send_blocking(EwsPasswordPrompt {
                endpoint: ews_config.url.clone(),
                user: ews_config.user.clone(),
                replacing_existing_password,
                response_sender,
            })
            .map_err(|e| CalendarError {
                msg: format!("Could not request EWS password from GUI thread: {}", e),
            })?;

        response_receiver
            .recv()
            .map_err(|e| CalendarError {
                msg: format!("Could not receive EWS password from GUI thread: {}", e),
            })?
            .ok_or_else(|| CalendarError {
                msg: "EWS password prompt was cancelled".to_string(),
            })
    }
}

fn get_ical(url: &str, user_agent: &str) -> Result<String, CalendarError> {
    log::debug!("fetching calendar data from ICS source");
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build();
    let agent: Agent = config.into();
    agent
        .get(url)
        .header("User-Agent", user_agent)
        .call()
        .map_err(|e| CalendarError {
            msg: format!("Error calling calendar URL: {}", e),
        })?
        .body_mut()
        .read_to_string()
        .map_err(|e| CalendarError {
            msg: format!("Error reading calendar response body: {}", e),
        })
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::get_ical;

    #[test]
    fn sends_configured_user_agent_to_ical_source() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.ends_with(b"\r\n\r\n") {
                let bytes_read = stream.read(&mut buffer).unwrap();
                assert_ne!(bytes_read, 0, "connection closed before request headers");
                request.extend_from_slice(&buffer[..bytes_read]);
            }

            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\ncalendar",
                )
                .unwrap();

            String::from_utf8_lossy(&request).into_owned()
        });

        let calendar = get_ical(
            &format!("http://{}/calendar.ics", address),
            "CustomCalendarClient/1.0",
        )
        .unwrap();

        assert_eq!(calendar, "calendar");
        let request = server.join().unwrap().to_ascii_lowercase();
        assert!(request.contains("user-agent: customcalendarclient/1.0\r\n"));
    }
}

fn map_ews_error(error: EwsError) -> CalendarError {
    match error {
        EwsError::AuthFailed => CalendarError {
            msg: "EWS authentication failed".to_string(),
        },
        EwsError::Other(msg) => CalendarError { msg },
    }
}
