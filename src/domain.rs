use chrono::prelude::*;
use chrono_tz::Tz;
use std::fmt;

// From https://doc.rust-lang.org/stable/rust-by-example/error/multiple_error_types/define_error_type.html and message added
#[derive(Debug, Clone)]
pub struct CalendarError {
    // Type "String" means that the struct owns and stores the string, if I would use a string reference (&str)
    // I would also need to specify a lifecycle like  "msg: &'a str,". This is less storage
    // but it means we can't just generate error messages on the fly that are not static
    // If I _would_ use a refernce we need to suffix the CalendarError type with a lifetime like
    // CalendarError<'a>
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
    pub num_participants: u32,
}
