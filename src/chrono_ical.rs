use crate::custom_timezone::CustomTz;
use chrono_tz::Tz;
use either::Either;
use either::Left;
use either::Right;
use std::collections::HashMap;

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
}

/// Parses a TZID string as it may occur in an ical event and returns a chrono-tz timezone.
/// This supports the following formats:
/// * If you provide a custom timezone map then that is checked first
/// * If no custom timezones are provided, it will defer to standardized timezone definitions
pub fn parse_tzid<'a>(
    tzid: &str,
    custom_timezones: &'a HashMap<String, CustomTz>,
) -> Result<Either<Tz, &'a CustomTz>, String> {
    match custom_timezones.get(tzid) {
        Some(tz) => {
            return Ok(Right(tz));
        }
        None => Ok(Left(parse_standard_tz(tzid)?)),
    }
}

/// Formats supported:
/// * Explicit timezone strings containing a UTC offset and some cities, e.g. "(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna"
/// * Windows specific timezone identifiers like "W. Europe Standard Time", these are sourced from https://github.com/unicode-org/cldr/blob/master/common/supplemental/windowsZones.xml
/// * IANA Timezone identifiers like "Europe/Berlin" (natively supported by chrono-tz)
pub fn parse_standard_tz(tzid: &str) -> Result<Tz, String> {
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
        assert_eq!(Berlin, parse_standard_tz("Europe/Berlin").unwrap());
    }

    #[test]
    fn parses_windows_strings() {
        assert_eq!(
            Berlin,
            parse_standard_tz("W. Europe Standard Time").unwrap()
        );
    }

    #[test]
    fn parses_fucked_windows_strings() {
        assert_eq!(
            Vienna,
            parse_standard_tz("(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna")
                .unwrap()
        );
        assert_eq!(
            Dublin,
            parse_standard_tz("(UTC+00:00) Dublin, Edinburgh, Lisbon, London").unwrap()
        );
    }
}
