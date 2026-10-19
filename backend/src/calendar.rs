//! Which days and hours the commute is watched.
//!
//! Office days follow a two-week cycle, minus French public holidays and time
//! away. Watch windows are in the user's local time, so the caller passes the
//! current instant and the time zone.

use jiff::civil::{Date, Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};

/// Office days as a two-week cycle, minus public holidays and time away.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calendar {
    /// Any day of a week that uses `first_week`. The weeks then alternate from there.
    pub anchor: Date,
    #[serde(deserialize_with = "weekdays")]
    pub first_week: Vec<Weekday>,
    #[serde(deserialize_with = "weekdays")]
    pub second_week: Vec<Weekday>,
    /// Whit Monday is a public holiday, but many employers make it a working "solidarity day".
    #[serde(default = "yes")]
    pub whit_monday_off: bool,
    /// Last day away from the office, included. Set from the app, not from the configuration file.
    #[serde(skip)]
    pub away_through: Option<Date>,
}

fn yes() -> bool {
    true
}

/// Weekdays by their English names, as in "monday".
fn weekdays<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Weekday>, D::Error> {
    Vec::<String>::deserialize(deserializer)?
        .iter()
        .map(|name| match name.to_lowercase().as_str() {
            "monday" => Ok(Weekday::Monday),
            "tuesday" => Ok(Weekday::Tuesday),
            "wednesday" => Ok(Weekday::Wednesday),
            "thursday" => Ok(Weekday::Thursday),
            "friday" => Ok(Weekday::Friday),
            "saturday" => Ok(Weekday::Saturday),
            "sunday" => Ok(Weekday::Sunday),
            _ => Err(D::Error::custom(format!("unknown weekday \"{name}\""))),
        })
        .collect()
}

/// A half-open span of local time: `start` is included, `end` is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub start: Time,
    pub end: Time,
    /// From then until `start`, trains are followed without notifying, so that
    /// trains cancelled early are known when the window opens.
    #[serde(default)]
    pub trains_from: Option<Time>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchWindows {
    pub outbound: Window,
    pub inbound: Window,
}

/// When each way is usually set off on, if said: a reminder tells when to
/// leave for the first train that can be caught then.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leave {
    pub outbound: Option<Time>,
    pub inbound: Option<Time>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Outbound,
    Inbound,
}

impl Calendar {
    pub fn is_office_day(&self, date: Date) -> bool {
        if self.away_through.is_some_and(|last| date <= last)
            || is_public_holiday(date, self.whit_monday_off)
        {
            return false;
        }
        // Count whole weeks from the anchor's Monday. ISO week numbers would not
        // do: a year with 53 weeks puts two odd weeks in a row.
        let anchor = self.anchor;
        let monday = anchor - i64::from(anchor.weekday().to_monday_zero_offset()).days();
        let weeks = (date - monday).get_days().div_euclid(7);
        let days = if weeks % 2 == 0 {
            &self.first_week
        } else {
            &self.second_week
        };
        days.contains(&date.weekday())
    }
}

/// Gregorian Easter Sunday (anonymous algorithm, Meeus/Jones/Butcher).
pub fn easter_sunday(year: i16) -> Date {
    let y = i32::from(year);
    let (a, b, c) = (y % 19, y / 100, y % 100);
    let (d, e) = (b / 4, b % 4);
    let g = (b - (b + 8) / 25 + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let (i, k) = (c / 4, c % 4);
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let n = h + l - 7 * m + 114;
    jiff::civil::date(year, (n / 31) as i8, (n % 31 + 1) as i8)
}

/// Public holidays of metropolitan France.
pub fn is_public_holiday(date: Date, whit_monday_off: bool) -> bool {
    const FIXED: [(i8, i8); 8] = [
        (1, 1),
        (5, 1),
        (5, 8),
        (7, 14),
        (8, 15),
        (11, 1),
        (11, 11),
        (12, 25),
    ];
    if FIXED.contains(&(date.month(), date.day())) {
        return true;
    }
    // Easter Monday, Ascension Thursday and Whit Monday.
    match (date - easter_sunday(date.year())).get_days() {
        1 | 39 => true,
        50 => whit_monday_off,
        _ => false,
    }
}

/// The direction being watched at `now`, if any.
pub fn watched_direction(
    now: Timestamp,
    tz: &TimeZone,
    calendar: &Calendar,
    windows: &WatchWindows,
) -> Option<Direction> {
    match phase(now, tz, calendar, windows) {
        Some((direction, Phase::Watching)) => Some(direction),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Before the window, from its `trains_from`: trains are followed, nothing is notified.
    Observing,
    Watching,
}

/// The direction followed at `now`, if any, and whether its window is open.
pub fn phase(
    now: Timestamp,
    tz: &TimeZone,
    calendar: &Calendar,
    windows: &WatchWindows,
) -> Option<(Direction, Phase)> {
    let local = now.to_zoned(tz.clone());
    if !calendar.is_office_day(local.date()) {
        return None;
    }
    let time = local.time();
    [
        (Direction::Outbound, windows.outbound),
        (Direction::Inbound, windows.inbound),
    ]
    .into_iter()
    .find_map(|(direction, window)| {
        if window.start <= time && time < window.end {
            Some((direction, Phase::Watching))
        } else if window.trains_from.is_some_and(|from| from <= time) && time < window.start {
            Some((direction, Phase::Observing))
        } else {
            None
        }
    })
}

/// The configured calendar and watch windows, in their time zone.
#[derive(Clone, Debug)]
pub struct Schedule {
    pub calendar: Calendar,
    pub windows: WatchWindows,
    pub time_zone: TimeZone,
}

impl Schedule {
    /// When the next window opens after `now`, with the time away set from the app.
    pub fn next_window(&self, now: Timestamp, away_through: Option<Date>) -> Option<Timestamp> {
        let calendar = Calendar {
            away_through,
            ..self.calendar.clone()
        };
        next_window(now, &self.time_zone, &calendar, &self.windows)
    }
}

/// How far `next_window` looks ahead: no time away lasts longer.
const LOOKAHEAD_DAYS: i64 = 366;

/// When the next watch window opens, strictly after `now`.
pub fn next_window(
    now: Timestamp,
    tz: &TimeZone,
    calendar: &Calendar,
    windows: &WatchWindows,
) -> Option<Timestamp> {
    let today = now.to_zoned(tz.clone()).date();
    let mut starts = [windows.outbound.start, windows.inbound.start];
    starts.sort();
    (0..LOOKAHEAD_DAYS)
        .map(|offset| today + offset.days())
        .filter(|day| calendar.is_office_day(*day))
        .flat_map(|day| starts.map(|start| day.to_datetime(start)))
        .filter_map(|start| start.to_zoned(tz.clone()).ok())
        .map(|start| start.timestamp())
        .find(|start| *start > now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::Weekday::{Friday, Monday, Thursday, Tuesday, Wednesday};
    use jiff::civil::{date, time};

    // An example commute, not anyone's real schedule: Tuesday and Thursday one
    // week, Monday, Wednesday and Friday the next.
    fn calendar() -> Calendar {
        Calendar {
            anchor: date(2026, 11, 4),
            first_week: vec![Tuesday, Thursday],
            second_week: vec![Monday, Wednesday, Friday],
            whit_monday_off: true,
            away_through: None,
        }
    }

    fn windows() -> WatchWindows {
        WatchWindows {
            outbound: Window {
                start: time(6, 45, 0, 0),
                end: time(8, 15, 0, 0),
                trains_from: None,
            },
            inbound: Window {
                start: time(16, 0, 0, 0),
                end: time(17, 30, 0, 0),
                trains_from: None,
            },
        }
    }

    fn paris() -> TimeZone {
        TimeZone::get("Europe/Paris").unwrap()
    }

    fn at(utc: &str) -> Timestamp {
        utc.parse().unwrap()
    }

    #[test]
    fn office_days_alternate_between_the_two_weeks() {
        let calendar = calendar();
        // Week of Monday 2 November 2026: first week.
        assert!(!calendar.is_office_day(date(2026, 11, 2)));
        assert!(calendar.is_office_day(date(2026, 11, 3)));
        assert!(calendar.is_office_day(date(2026, 11, 5)));
        assert!(!calendar.is_office_day(date(2026, 11, 6)));
        // Week of Monday 9 November: second week (Wednesday 11 November is a holiday).
        assert!(calendar.is_office_day(date(2026, 11, 9)));
        assert!(!calendar.is_office_day(date(2026, 11, 10)));
        assert!(calendar.is_office_day(date(2026, 11, 13)));
        // Week of Monday 16 November: first week again.
        assert!(calendar.is_office_day(date(2026, 11, 17)));
        assert!(!calendar.is_office_day(date(2026, 11, 16)));
    }

    #[test]
    fn weeks_before_the_anchor_alternate_too() {
        let calendar = calendar();
        // Week of 26 October: second week. Week of 19 October: first week.
        assert!(calendar.is_office_day(date(2026, 10, 26)));
        assert!(!calendar.is_office_day(date(2026, 10, 27)));
        assert!(calendar.is_office_day(date(2026, 10, 20)));
        assert!(!calendar.is_office_day(date(2026, 10, 19)));
    }

    #[test]
    fn alternation_ignores_iso_week_numbers() {
        // 2026 has 53 ISO weeks: weeks 53 and 1 are both odd, but must still alternate.
        let calendar = Calendar {
            anchor: date(2026, 12, 28),
            ..calendar()
        };
        assert!(calendar.is_office_day(date(2026, 12, 29)));
        assert!(!calendar.is_office_day(date(2027, 1, 5)));
        assert!(calendar.is_office_day(date(2027, 1, 6)));
        assert!(calendar.is_office_day(date(2027, 1, 12)));
    }

    #[test]
    fn weekdays_are_read_by_their_english_names() {
        #[derive(Deserialize)]
        struct Days {
            #[serde(deserialize_with = "weekdays")]
            days: Vec<Weekday>,
        }
        let all = r#"{"days": ["Monday", "tuesday", "WEDNESDAY", "thursday", "friday", "saturday", "sunday"]}"#;
        let days: Days = serde_json::from_str(all).unwrap();
        use Weekday::{Saturday, Sunday};
        assert_eq!(
            days.days,
            [
                Monday, Tuesday, Wednesday, Thursday, Friday, Saturday, Sunday
            ]
        );
        assert!(serde_json::from_str::<Days>(r#"{"days": "monday"}"#).is_err());
    }

    #[test]
    fn no_office_on_a_public_holiday() {
        // Wednesday 11 November 2026 falls in a second week.
        assert!(!calendar().is_office_day(date(2026, 11, 11)));
    }

    #[test]
    fn no_office_while_away_last_day_included() {
        let calendar = Calendar {
            away_through: Some(date(2026, 11, 9)),
            ..calendar()
        };
        assert!(!calendar.is_office_day(date(2026, 11, 3)));
        assert!(!calendar.is_office_day(date(2026, 11, 9)));
        assert!(calendar.is_office_day(date(2026, 11, 13)));
    }

    #[test]
    fn easter_sunday_matches_published_dates() {
        let published = [
            (2024, date(2024, 3, 31)),
            (2025, date(2025, 4, 20)),
            (2026, date(2026, 4, 5)),
            (2027, date(2027, 3, 28)),
            (2028, date(2028, 4, 16)),
            (2029, date(2029, 4, 1)),
            (2030, date(2030, 4, 21)),
            (2035, date(2035, 3, 25)),
            (2038, date(2038, 4, 25)),
        ];
        for (year, expected) in published {
            assert_eq!(easter_sunday(year), expected, "{year}");
        }
    }

    #[test]
    fn french_public_holidays_of_2026() {
        // Official list for 2026.
        let holidays = [
            date(2026, 1, 1),
            date(2026, 4, 6),
            date(2026, 5, 1),
            date(2026, 5, 8),
            date(2026, 5, 14),
            date(2026, 5, 25),
            date(2026, 7, 14),
            date(2026, 8, 15),
            date(2026, 11, 1),
            date(2026, 11, 11),
            date(2026, 12, 25),
        ];
        for holiday in holidays {
            assert!(is_public_holiday(holiday, true), "{holiday}");
        }
        let mut count = 0;
        let mut day = date(2026, 1, 1);
        while day.year() == 2026 {
            count += usize::from(is_public_holiday(day, true));
            day = day.tomorrow().unwrap();
        }
        assert_eq!(count, holidays.len());
    }

    #[test]
    fn whit_monday_can_be_a_working_day() {
        assert!(is_public_holiday(date(2027, 5, 17), true));
        assert!(!is_public_holiday(date(2027, 5, 17), false));
        // Easter Monday and Ascension stay holidays.
        assert!(is_public_holiday(date(2027, 3, 29), false));
        assert!(is_public_holiday(date(2027, 5, 6), false));
    }

    #[test]
    fn trains_are_observed_before_a_window_that_asks_for_it() {
        let tz = paris();
        let calendar = calendar();
        let mut windows = windows();
        // Without `trains_from`, nothing happens before the window.
        assert_eq!(
            phase(at("2026-11-03T05:30:00Z"), &tz, &calendar, &windows),
            None
        );
        windows.outbound.trains_from = Some(time(5, 30, 0, 0));
        let phase_at = |utc| phase(at(utc), &tz, &calendar, &windows);
        let observing = Some((Direction::Outbound, Phase::Observing));
        assert_eq!(phase_at("2026-11-03T04:29:59Z"), None);
        assert_eq!(phase_at("2026-11-03T04:30:00Z"), observing);
        assert_eq!(phase_at("2026-11-03T05:44:59Z"), observing);
        assert_eq!(
            phase_at("2026-11-03T05:45:00Z"),
            Some((Direction::Outbound, Phase::Watching))
        );
        assert_eq!(phase_at("2026-11-03T07:15:00Z"), None);
        assert_eq!(
            phase_at("2026-11-03T15:00:00Z"),
            Some((Direction::Inbound, Phase::Watching))
        );
        // Nothing on a day off.
        assert_eq!(
            phase(at("2026-11-02T04:30:00Z"), &tz, &calendar, &windows),
            None
        );
    }

    #[test]
    fn watched_direction_follows_local_time() {
        let (calendar, windows, tz) = (calendar(), windows(), paris());
        // Tuesday 3 November 2026, Paris is UTC+1.
        assert_eq!(
            watched_direction(at("2026-11-03T05:44:59Z"), &tz, &calendar, &windows),
            None
        );
        assert_eq!(
            watched_direction(at("2026-11-03T05:45:00Z"), &tz, &calendar, &windows),
            Some(Direction::Outbound)
        );
        assert_eq!(
            watched_direction(at("2026-11-03T07:14:59Z"), &tz, &calendar, &windows),
            Some(Direction::Outbound)
        );
        assert_eq!(
            watched_direction(at("2026-11-03T07:15:00Z"), &tz, &calendar, &windows),
            None
        );
        assert_eq!(
            watched_direction(at("2026-11-03T15:00:00Z"), &tz, &calendar, &windows),
            Some(Direction::Inbound)
        );
        assert_eq!(
            watched_direction(at("2026-11-03T16:30:00Z"), &tz, &calendar, &windows),
            None
        );
    }

    #[test]
    fn watched_direction_across_the_change_to_winter_time() {
        let (calendar, windows, tz) = (calendar(), windows(), paris());
        // Tuesday 20 October 2026, before 25 October: Paris is UTC+2.
        assert_eq!(
            watched_direction(at("2026-10-20T04:45:00Z"), &tz, &calendar, &windows),
            Some(Direction::Outbound)
        );
        // Monday 26 October, after the change: UTC+1.
        assert_eq!(
            watched_direction(at("2026-10-26T04:45:00Z"), &tz, &calendar, &windows),
            None
        );
        assert_eq!(
            watched_direction(at("2026-10-26T05:45:00Z"), &tz, &calendar, &windows),
            Some(Direction::Outbound)
        );
    }

    #[test]
    fn nothing_is_watched_on_a_day_off() {
        let (calendar, windows, tz) = (calendar(), windows(), paris());
        // Monday 2 November 2026 is not an office day in a first week.
        assert_eq!(
            watched_direction(at("2026-11-02T06:00:00Z"), &tz, &calendar, &windows),
            None
        );
    }

    #[test]
    fn the_next_window_is_the_next_start_on_an_office_day() {
        let (calendar, windows, tz) = (calendar(), windows(), paris());
        let next = |now: &str| next_window(at(now), &tz, &calendar, &windows);
        // Tuesday 3 November, 7:00 in Paris, during the morning window: next is 16:00.
        assert_eq!(
            next("2026-11-03T06:00:00Z"),
            Some(at("2026-11-03T15:00:00Z"))
        );
        // At the very start of a window, that window is not the next one.
        assert_eq!(
            next("2026-11-03T05:45:00Z"),
            Some(at("2026-11-03T15:00:00Z"))
        );
        // Tuesday evening: Wednesday is not an office day, Thursday is.
        assert_eq!(
            next("2026-11-03T17:00:00Z"),
            Some(at("2026-11-05T05:45:00Z"))
        );
    }

    #[test]
    fn the_next_window_skips_time_away_and_holidays() {
        let (windows, tz) = (windows(), paris());
        let calendar = Calendar {
            away_through: Some(date(2026, 11, 9)),
            ..calendar()
        };
        // From Thursday 5 November: away on Monday 9, Wednesday 11 is a holiday, Friday 13 it is.
        assert_eq!(
            next_window(at("2026-11-05T17:00:00Z"), &tz, &calendar, &windows),
            Some(at("2026-11-13T05:45:00Z"))
        );
    }

    #[test]
    fn the_next_window_is_in_local_time_across_the_change_to_winter_time() {
        let (calendar, windows, tz) = (calendar(), windows(), paris());
        // Friday 23 October, in summer time; Monday 26 October, 6:45 in winter time.
        assert_eq!(
            next_window(at("2026-10-23T16:00:00Z"), &tz, &calendar, &windows),
            Some(at("2026-10-26T05:45:00Z"))
        );
    }

    #[test]
    fn the_schedule_takes_the_time_away_it_is_given() {
        let schedule = Schedule {
            calendar: calendar(),
            windows: windows(),
            time_zone: paris(),
        };
        // From Tuesday 3 November, 18:00 in Paris.
        let evening = at("2026-11-03T17:00:00Z");
        assert_eq!(
            schedule.next_window(evening, None),
            Some(at("2026-11-05T05:45:00Z"))
        );
        // Away through Thursday: Monday 9 November.
        assert_eq!(
            schedule.next_window(evening, Some(date(2026, 11, 5))),
            Some(at("2026-11-09T05:45:00Z"))
        );
    }

    #[test]
    fn without_office_days_there_is_no_next_window() {
        let calendar = Calendar {
            first_week: vec![],
            second_week: vec![],
            ..calendar()
        };
        assert_eq!(
            next_window(at("2026-11-03T06:00:00Z"), &paris(), &calendar, &windows()),
            None
        );
    }
}
