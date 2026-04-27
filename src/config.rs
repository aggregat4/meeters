use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use chrono_tz::Tz;
use directories::ProjectDirs;

/// Time between two ical calendar download in milliseconds
const DEFAULT_POLLING_INTERVAL_MS: u128 = 2 * 60 * 1000;
/// The amount of time in seconds we want to be warned before the meeting starts
const DEFAULT_EVENT_WARNING_TIME_SECONDS: i64 = 60;
/// Default start hour for the timeline view (8 AM)
const DEFAULT_START_HOUR: i32 = 8;
/// Default end hour for the timeline view (8 PM)
const DEFAULT_END_HOUR: i32 = 20;
/// Default number of future days to show (1 = today + tomorrow)
const DEFAULT_FUTURE_DAYS: i32 = 1;
const DEFAULT_LOCAL_TIMEZONE: &str = "Europe/Berlin";

#[derive(Debug)]
pub struct Config {
    pub local_tz_iana: String,
    pub local_tz: Tz,
    pub ical_url: String,
    pub show_event_notification: bool,
    pub use_zoommtg: bool,
    pub polling_interval_ms: u128,
    pub event_warning_time_seconds: i64,
    pub start_hour: i32,
    pub end_hour: i32,
    pub future_days: i32,
}

#[derive(Debug)]
pub struct ConfigError {
    msg: String,
}

impl ConfigError {
    fn new(msg: impl Into<String>) -> Self {
        ConfigError { msg: msg.into() }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Configuration error: {}", self.msg)
    }
}

impl Error for ConfigError {}

pub fn get_config_directory() -> PathBuf {
    ProjectDirs::from("net", "aggregat4", "meeters")
        .expect("Project directory must be available")
        .config_dir()
        .to_path_buf()
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let config_file = get_config_directory().join("meeters_config.env");
        if config_file.exists() {
            dotenvy::from_path(&config_file).map_err(|e| {
                ConfigError::new(format!(
                    "Can not load configuration file {}: {}",
                    config_file.display(),
                    e
                ))
            })?;
        }

        Self::from_lookup(|name| dotenvy::var(name).ok())
    }

    fn from_lookup<F>(mut lookup: F) -> Result<Self, ConfigError>
    where
        F: FnMut(&str) -> Option<String>,
    {
        let local_tz_iana =
            lookup("MEETERS_LOCAL_TIMEZONE").unwrap_or_else(|| DEFAULT_LOCAL_TIMEZONE.to_string());
        let local_tz = local_tz_iana.parse::<Tz>().map_err(|e| {
            ConfigError::new(format!(
                "MEETERS_LOCAL_TIMEZONE must be a valid IANA timezone identifier, got '{}': {}",
                local_tz_iana, e
            ))
        })?;

        let ical_url = lookup("MEETERS_ICAL_URL")
            .ok_or_else(|| ConfigError::new("MEETERS_ICAL_URL is required"))?;

        let show_event_notification = parse_bool(
            lookup("MEETERS_EVENT_NOTIFICATION"),
            "MEETERS_EVENT_NOTIFICATION",
            true,
        )?;
        let use_zoommtg = parse_bool(lookup("MEETERS_USE_ZOOMMTG"), "MEETERS_USE_ZOOMMTG", false)?;
        let polling_interval_ms = parse_u128(
            lookup("MEETERS_POLLING_INTERVAL_MS"),
            "MEETERS_POLLING_INTERVAL_MS",
            DEFAULT_POLLING_INTERVAL_MS,
        )?;
        let event_warning_time_seconds = parse_i64(
            lookup("MEETERS_EVENT_WARNING_TIME_SECONDS"),
            "MEETERS_EVENT_WARNING_TIME_SECONDS",
            DEFAULT_EVENT_WARNING_TIME_SECONDS,
        )?;
        let start_hour = parse_i32(
            lookup("MEETERS_TODAY_START_HOUR"),
            "MEETERS_TODAY_START_HOUR",
            DEFAULT_START_HOUR,
        )?;
        let end_hour = parse_i32(
            lookup("MEETERS_TODAY_END_HOUR"),
            "MEETERS_TODAY_END_HOUR",
            DEFAULT_END_HOUR,
        )?;
        let future_days = parse_i32(
            lookup("MEETERS_FUTURE_DAYS"),
            "MEETERS_FUTURE_DAYS",
            DEFAULT_FUTURE_DAYS,
        )?;

        validate_positive("MEETERS_POLLING_INTERVAL_MS", polling_interval_ms)?;
        validate_non_negative_i64(
            "MEETERS_EVENT_WARNING_TIME_SECONDS",
            event_warning_time_seconds,
        )?;
        validate_hour("MEETERS_TODAY_START_HOUR", start_hour)?;
        validate_hour("MEETERS_TODAY_END_HOUR", end_hour)?;
        if start_hour >= end_hour {
            return Err(ConfigError::new(format!(
                "MEETERS_TODAY_START_HOUR ({}) must be smaller than MEETERS_TODAY_END_HOUR ({})",
                start_hour, end_hour
            )));
        }
        validate_non_negative_i32("MEETERS_FUTURE_DAYS", future_days)?;

        Ok(Config {
            local_tz_iana,
            local_tz,
            ical_url,
            show_event_notification,
            use_zoommtg,
            polling_interval_ms,
            event_warning_time_seconds,
            start_hour,
            end_hour,
            future_days,
        })
    }
}

fn parse_bool(value: Option<String>, name: &str, default_value: bool) -> Result<bool, ConfigError> {
    value
        .map(|value| {
            value.parse::<bool>().map_err(|_| {
                ConfigError::new(format!(
                    "{} must be a boolean value ('true' or 'false'), got '{}'",
                    name, value
                ))
            })
        })
        .unwrap_or(Ok(default_value))
}

fn parse_u128(value: Option<String>, name: &str, default_value: u128) -> Result<u128, ConfigError> {
    value
        .map(|value| {
            value.parse::<u128>().map_err(|_| {
                ConfigError::new(format!(
                    "{} must be a positive integer, got '{}'",
                    name, value
                ))
            })
        })
        .unwrap_or(Ok(default_value))
}

fn parse_i64(value: Option<String>, name: &str, default_value: i64) -> Result<i64, ConfigError> {
    value
        .map(|value| {
            value.parse::<i64>().map_err(|_| {
                ConfigError::new(format!("{} must be an integer, got '{}'", name, value))
            })
        })
        .unwrap_or(Ok(default_value))
}

fn parse_i32(value: Option<String>, name: &str, default_value: i32) -> Result<i32, ConfigError> {
    value
        .map(|value| {
            value.parse::<i32>().map_err(|_| {
                ConfigError::new(format!("{} must be an integer, got '{}'", name, value))
            })
        })
        .unwrap_or(Ok(default_value))
}

fn validate_positive(name: &str, value: u128) -> Result<(), ConfigError> {
    if value == 0 {
        Err(ConfigError::new(format!("{} must be greater than 0", name)))
    } else {
        Ok(())
    }
}

fn validate_non_negative_i64(name: &str, value: i64) -> Result<(), ConfigError> {
    if value < 0 {
        Err(ConfigError::new(format!(
            "{} must be greater than or equal to 0",
            name
        )))
    } else {
        Ok(())
    }
}

fn validate_non_negative_i32(name: &str, value: i32) -> Result<(), ConfigError> {
    if value < 0 {
        Err(ConfigError::new(format!(
            "{} must be greater than or equal to 0",
            name
        )))
    } else {
        Ok(())
    }
}

fn validate_hour(name: &str, value: i32) -> Result<(), ConfigError> {
    if (0..=23).contains(&value) {
        Ok(())
    } else {
        Err(ConfigError::new(format!(
            "{} must be between 0 and 23, got {}",
            name, value
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config_from_values(values: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let values: HashMap<String, String> = values
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        Config::from_lookup(|name| values.get(name).cloned())
    }

    #[test]
    fn loads_defaults_for_optional_values() {
        let config =
            config_from_values(&[("MEETERS_ICAL_URL", "https://example.com/calendar.ics")])
                .unwrap();

        assert_eq!(config.local_tz_iana, "Europe/Berlin");
        assert_eq!(config.ical_url, "https://example.com/calendar.ics");
        assert!(config.show_event_notification);
        assert!(!config.use_zoommtg);
        assert_eq!(config.polling_interval_ms, DEFAULT_POLLING_INTERVAL_MS);
        assert_eq!(
            config.event_warning_time_seconds,
            DEFAULT_EVENT_WARNING_TIME_SECONDS
        );
        assert_eq!(config.start_hour, DEFAULT_START_HOUR);
        assert_eq!(config.end_hour, DEFAULT_END_HOUR);
        assert_eq!(config.future_days, DEFAULT_FUTURE_DAYS);
    }

    #[test]
    fn loads_valid_overrides() {
        let config = config_from_values(&[
            ("MEETERS_ICAL_URL", "https://example.com/calendar.ics"),
            ("MEETERS_LOCAL_TIMEZONE", "UTC"),
            ("MEETERS_EVENT_NOTIFICATION", "false"),
            ("MEETERS_USE_ZOOMMTG", "true"),
            ("MEETERS_POLLING_INTERVAL_MS", "60000"),
            ("MEETERS_EVENT_WARNING_TIME_SECONDS", "120"),
            ("MEETERS_TODAY_START_HOUR", "7"),
            ("MEETERS_TODAY_END_HOUR", "18"),
            ("MEETERS_FUTURE_DAYS", "3"),
        ])
        .unwrap();

        assert_eq!(config.local_tz_iana, "UTC");
        assert!(!config.show_event_notification);
        assert!(config.use_zoommtg);
        assert_eq!(config.polling_interval_ms, 60000);
        assert_eq!(config.event_warning_time_seconds, 120);
        assert_eq!(config.start_hour, 7);
        assert_eq!(config.end_hour, 18);
        assert_eq!(config.future_days, 3);
    }

    #[test]
    fn requires_ical_url() {
        let error = config_from_values(&[]).unwrap_err();
        assert!(error.to_string().contains("MEETERS_ICAL_URL is required"));
    }

    #[test]
    fn rejects_invalid_boolean() {
        let error = config_from_values(&[
            ("MEETERS_ICAL_URL", "https://example.com/calendar.ics"),
            ("MEETERS_EVENT_NOTIFICATION", "yes"),
        ])
        .unwrap_err();

        assert!(error.to_string().contains("MEETERS_EVENT_NOTIFICATION"));
        assert!(error.to_string().contains("boolean"));
    }

    #[test]
    fn rejects_invalid_hour_ranges() {
        let error = config_from_values(&[
            ("MEETERS_ICAL_URL", "https://example.com/calendar.ics"),
            ("MEETERS_TODAY_START_HOUR", "20"),
            ("MEETERS_TODAY_END_HOUR", "8"),
        ])
        .unwrap_err();

        assert!(error.to_string().contains("must be smaller"));
    }

    #[test]
    fn rejects_hours_outside_day() {
        let error = config_from_values(&[
            ("MEETERS_ICAL_URL", "https://example.com/calendar.ics"),
            ("MEETERS_TODAY_START_HOUR", "24"),
        ])
        .unwrap_err();

        assert!(error.to_string().contains("between 0 and 23"));
    }

    #[test]
    fn rejects_negative_future_days() {
        let error = config_from_values(&[
            ("MEETERS_ICAL_URL", "https://example.com/calendar.ics"),
            ("MEETERS_FUTURE_DAYS", "-1"),
        ])
        .unwrap_err();

        assert!(error.to_string().contains("MEETERS_FUTURE_DAYS"));
        assert!(error.to_string().contains("greater than or equal to 0"));
    }
}
