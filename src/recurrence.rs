//! Recurrence engines operate on calendar fields. Meeters owns timezone
//! resolution, exclusions and overrides; UTC here is only a field carrier.
use crate::domain::CalendarError;
use chrono::TimeZone;
use chrono::{DateTime, Datelike};
use chrono_tz::Tz;
use chrono_tz::UTC;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Engine {
    #[cfg(test)]
    Fork,
    Upstream,
}

impl Engine {
    /// Expand up to (but excluding) `end_year`. Iterate rather than calling
    /// all(), so a recurrence without UNTIL/COUNT stops at the application window.
    pub(crate) fn expand(
        self,
        input: &str,
        end_year: i32,
    ) -> Result<Vec<DateTime<Tz>>, CalendarError> {
        let error = |e| CalendarError {
            msg: format!("error in RRULE parsing: {e}"),
        };
        match self {
            #[cfg(test)]
            Self::Fork => {
                let set = input
                    .parse::<rrule_fork::RRuleSet>()
                    .map_err(|e| error(e.to_string()))?;
                Ok(set
                    .into_iter()
                    .take_while(|d| d.year() < end_year)
                    .collect())
            }
            Self::Upstream => {
                let input = upstream_input(input)?;
                let set = input
                    .parse::<rrule::RRuleSet>()
                    .map_err(|e| error(e.to_string()))?;
                Ok((&set)
                    .into_iter()
                    .take_while(|d| d.year() < end_year)
                    .map(|d| UTC.from_utc_datetime(&d.naive_local()))
                    .collect())
            }
        }
    }
}

/// Upstream defaults floating dates to the host timezone. Explicit UTC avoids
/// that: the fields are already in the source timezone, not real UTC instants.
/// DATE values are represented as midnight, matching the fork's date expansion.
fn upstream_input(input: &str) -> Result<String, CalendarError> {
    fn stamp(value: &str) -> String {
        if value.len() == 8 {
            format!("{value}T000000Z")
        } else if value.ends_with('Z') {
            value.into()
        } else {
            format!("{value}Z")
        }
    }
    let mut start = None;
    let mut rules = Vec::new();
    for line in input.lines() {
        let (head, value) = line.split_once(':').ok_or_else(|| CalendarError {
            msg: "Invalid recurrence content line".into(),
        })?;
        match head.split(';').next().unwrap() {
            "DTSTART" => start = Some(format!("DTSTART:{}", stamp(value))),
            "RRULE" => rules.push(format!(
                "RRULE:{}",
                value
                    .split(';')
                    .map(|part| {
                        if let Some(until) = part.strip_prefix("UNTIL=") {
                            format!("UNTIL={}", stamp(until))
                        } else {
                            part.into()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(";")
            )),
            _ => {
                return Err(CalendarError {
                    msg: "Unexpected property in recurrence adapter".into(),
                })
            }
        }
    }
    let start = start.ok_or_else(|| CalendarError {
        msg: "Missing recurrence DTSTART".into(),
    })?;
    Ok(std::iter::once(start)
        .chain(rules)
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_compatibility_legacy_regressions() {
        for input in [
            "DTSTART;VALUE=DATE:20200812\nRRULE:FREQ=WEEKLY;UNTIL=20210511T220000Z;INTERVAL=1;BYDAY=WE;WKST=MO",
            "DTSTART;VALUE=DATE:20200701\nRRULE:FREQ=MONTHLY;UNTIL=20210303T090000Z;INTERVAL=1;BYDAY=1WE",
            "DTSTART;VALUE=DATE:20201230T130000\nRRULE:FREQ=MONTHLY;UNTIL=20210825T120000Z;INTERVAL=1;BYDAY=-1WE",
            "DTSTART:20210316T093000Z\nRRULE:FREQ=WEEKLY;UNTIL=20210511T093000Z;INTERVAL=1;BYDAY=TU;WKST=MO",
            "DTSTART;VALUE=DATE:20211206\nRRULE:FREQ=YEARLY;UNTIL=20221205T230000Z;INTERVAL=1;BYMONTHDAY=6;BYMONTH=12",
        ] {
            let fork = Engine::Fork.expand(input, 2024).unwrap();
            let upstream = Engine::Upstream.expand(input, 2024).unwrap();
            assert!(!fork.is_empty());
            assert_eq!(fork, upstream, "legacy input: {input}");
        }
    }

    #[test]
    fn calendar_compatibility_unending_rule_stops_at_window() {
        for engine in [Engine::Fork, Engine::Upstream] {
            let dates = engine
                .expand("DTSTART:20240101T090000Z\nRRULE:FREQ=YEARLY", 2027)
                .unwrap();
            let dates: Vec<_> = dates.iter().map(|d| d.to_rfc3339()).collect();
            assert_eq!(
                dates,
                [
                    "2024-01-01T09:00:00+00:00",
                    "2025-01-01T09:00:00+00:00",
                    "2026-01-01T09:00:00+00:00"
                ]
            );
        }
    }

    #[test]
    fn calendar_compatibility_invalid_rules_are_rejected() {
        for input in [
            "DTSTART:20240101T090000Z\nRRULE:FREQ=INVALID",
            "DTSTART:invalid\nRRULE:FREQ=DAILY;COUNT=2",
        ] {
            for engine in [Engine::Fork, Engine::Upstream] {
                assert!(
                    engine.expand(input, 2027).is_err(),
                    "{engine:?} accepted {input}"
                );
            }
        }
    }
    #[test]
    fn calendar_compatibility_known_invalid_byday_difference() {
        let input = "DTSTART:20240101T090000Z\nRRULE:FREQ=WEEKLY;BYDAY=INVALID;COUNT=2";
        // Characterize a discovered difference; catching the fork panic is
        // confined to this test, not added to production as an error policy.
        let fork = std::panic::catch_unwind(|| Engine::Fork.expand(input, 2027));
        assert!(
            fork.is_err(),
            "Update the comparison report if the fork is fixed"
        );
        assert!(Engine::Upstream.expand(input, 2027).is_err());
    }
}
