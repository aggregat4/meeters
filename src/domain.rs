use chrono::prelude::*;
use chrono_tz::Tz;
use std::collections::VecDeque;
use std::fmt;

pub const ONLINE_MEETING_MARKER: &str = " ◉";

#[derive(Debug, Clone)]
pub struct CalendarError {
    // Own the message so callers can construct contextual errors dynamically.
    pub msg: String,
}

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Calendar Error: {}", self.msg)
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub summary: String,
    pub description: String,
    pub location: String,
    pub meeturl: Option<String>,
    pub all_day: bool,
    pub start_timestamp: DateTime<Tz>,
    pub end_timestamp: DateTime<Tz>,
}

#[derive(Debug, Clone)]
pub struct RefreshLogEntry {
    pub timestamp: DateTime<Local>,
    pub successful: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct RefreshState {
    pub last_attempt_at: Option<DateTime<Local>>,
    pub last_success_at: Option<DateTime<Local>>,
    pub last_error: Option<String>,
    pub last_update_successful: Option<bool>,
    pub log_entries: VecDeque<RefreshLogEntry>,
    max_entries: usize,
}

impl RefreshState {
    pub fn new(max_entries: usize) -> Self {
        RefreshState {
            last_attempt_at: None,
            last_success_at: None,
            last_error: None,
            last_update_successful: None,
            log_entries: VecDeque::with_capacity(max_entries),
            max_entries,
        }
    }

    pub fn record_success(&mut self, event_count: usize) {
        let timestamp = Local::now();
        self.last_attempt_at = Some(timestamp);
        self.last_success_at = Some(timestamp);
        self.last_error = None;
        self.last_update_successful = Some(true);
        self.push_log_entry(RefreshLogEntry {
            timestamp,
            successful: true,
            message: format!(
                "Fetched and parsed calendar successfully ({} events).",
                event_count
            ),
        });
    }

    pub fn record_failure(&mut self, error: impl Into<String>) {
        let timestamp = Local::now();
        let error = error.into();
        self.last_attempt_at = Some(timestamp);
        self.last_error = Some(error.clone());
        self.last_update_successful = Some(false);
        self.push_log_entry(RefreshLogEntry {
            timestamp,
            successful: false,
            message: format!("Failed to fetch or parse calendar: {}", error),
        });
    }

    fn push_log_entry(&mut self, entry: RefreshLogEntry) {
        if self.log_entries.len() == self.max_entries {
            self.log_entries.pop_front();
        }
        self.log_entries.push_back(entry);
    }
}
