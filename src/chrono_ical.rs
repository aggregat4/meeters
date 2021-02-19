use chrono_tz::Tz;

use crate::chrono_windows_timezones::*;

fn parse_windows_tzid(tzid: &str) -> Result<Tz, String> {
    match WINDOWS_TZ_TO_CHRONO_TZ.get(tzid) {
        Some(tz) => Ok(*tz),
        None => Err(format!(
            "Timezone string {} does not represent a windows timezone",
            tzid
        )),
    }
}

/// Parses timezone of the form "(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna"
/// by identifying the UTC offset and generating a Tz from that
fn parse_explicit_tzid(tzid: &str) -> Result<Tz, String> {
    match FUCKED_WINDOWS_TZ_TO_CHRONO_TZ.get(tzid) {
        Some(tz) => Ok(*tz),
        None => Err(format!(
            "Timezone string {} does not represent a windows timezone",
            tzid
        )),
    }
    // lazy_static! {
    //     static ref EXPLICIT_TZ_ID_REGEX: regex::Regex =
    //         Regex::new(r"\(UTC([\+-][0-9][0-9]):([0-9][0-9])\)").unwrap();
    // }
    // match EXPLICIT_TZ_ID_REGEX.captures(tzid) {
    //     Some(captures) => {
    //         let hours = captures.get(1).unwrap().as_str().parse::<i32>().unwrap();
    //         let minutes = captures.get(2).unwrap().as_str().parse::<i32>().unwrap();
    //         let one_hour_in_secs = 3600;
    //         let one_minute_in_secs = 60;
    //         Ok(FixedOffset::east(
    //             hours * one_hour_in_secs + one_minute_in_secs * minutes,
    //         ))
    //     }
    //     None => Err(format!(
    //         "Can not find an explicit timezone string in {}",
    //         tzid
    //     )),
    // }
}

/// Parses a TZID string as it may occur in an ical event and returns a chrono-tz timezone.
/// This supports the following formats:
/// * Explicit timezone strings containing a UTC offset and some cities, e.g. "(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna"
/// * Windows specific timezone identifiers like "W. Europe Standard Time", these are sourced from https://github.com/unicode-org/cldr/blob/master/common/supplemental/windowsZones.xml
/// * IANA Timezone identifiers like "Europe/Berlin" (natively supported by chrono-tz)
pub fn parse_tzid(tzid: &str) -> Result<Tz, String> {
    // TODO: this is a rediculous form, should be using or_else or something but couldn't get it to
    // work
    match tzid.parse() {
        Ok(tz) => Ok(tz),
        Err(_) => match parse_windows_tzid(tzid) {
            Ok(wtz) => Ok(wtz),
            Err(_) => match parse_explicit_tzid(tzid) {
                Ok(etz) => Ok(etz),
                Err(_) => Err(format!("Can't parse tzid {}", tzid)),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::{Berlin, Dublin, Vienna};

    #[test]
    fn parses_iana_strings() {
        assert_eq!(Berlin, parse_tzid("Europe/Berlin").unwrap());
    }

    #[test]
    fn parses_windows_strings() {
        assert_eq!(Berlin, parse_tzid("W. Europe Standard Time").unwrap());
    }

    #[test]
    fn parses_fucked_windows_strings() {
        assert_eq!(
            Vienna,
            parse_tzid("(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna").unwrap()
        );
        assert_eq!(
            Dublin,
            parse_tzid("(UTC+00:00) Dublin, Edinburgh, Lisbon, London").unwrap()
        );
    }
}
