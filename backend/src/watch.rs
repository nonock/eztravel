//! Whether the commute is really watched, so that no news can be read as good news.
//!
//! A poll is complete when PRIM was read and every notification it decided
//! was sent. When no poll has been complete for `OUTAGE_AFTER` during a
//! window, the phone is told once, and told again when polls complete anew.
//! The app's home screen reads the same thing through `GET /status`.

use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::calendar::Direction;
use crate::school::Holiday;

/// Five polls in a row: long enough to ride out a hiccup, short enough to matter on the way.
pub const OUTAGE_AFTER: SignedDuration = SignedDuration::from_mins(10);

/// What keeps polls from being complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cause {
    /// PRIM cannot be read.
    Prim,
    /// Notifications cannot be sent.
    Firebase,
    /// The backend itself: it stopped polling, or has no device to notify.
    Server,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Outage {
    /// The last complete poll, or the start of the window if later.
    pub since: Timestamp,
    pub cause: Cause,
}

/// How polls went. Kept between polls, and across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Health {
    /// The window being watched, and its first poll.
    window: Option<(Date, Direction, Timestamp)>,
    /// The last complete poll.
    checked_at: Option<Timestamp>,
    /// What failed since then.
    failure: Option<Cause>,
    /// The outage was notified in the current window.
    outage_sent: bool,
    /// An outage was notified, in this window or an earlier one, and its end not yet.
    unresolved: bool,
}

impl Health {
    /// A poll in a window. A new window starts afresh, with time to check.
    pub fn watching(&mut self, now: Timestamp, today: Date, direction: Direction) {
        let current = self.window.map(|(day, known, _)| (day, known));
        if current != Some((today, direction)) {
            *self = Health {
                window: Some((today, direction, now)),
                checked_at: self.checked_at,
                unresolved: self.unresolved,
                ..Health::default()
            };
        }
    }

    /// Outside the windows: nothing is watched, so nothing is out.
    pub fn idle(&mut self) {
        *self = Health {
            checked_at: self.checked_at,
            unresolved: self.unresolved,
            ..Health::default()
        };
    }

    /// A complete poll.
    pub fn checked(&mut self, now: Timestamp) {
        self.checked_at = Some(now);
        self.failure = None;
        self.outage_sent = false;
        self.unresolved = false;
    }

    /// A failed poll. True when the outage is to be notified now.
    pub fn failed(&mut self, now: Timestamp, cause: Cause) -> bool {
        self.failure = Some(cause);
        !self.outage_sent && self.outage(now).is_some()
    }

    pub fn outage_sent(&mut self) {
        self.outage_sent = true;
        self.unresolved = true;
    }

    /// True when an outage was notified: the next complete poll says it is over,
    /// even in a later window.
    pub fn recovering(&self) -> bool {
        self.unresolved
    }

    pub fn checked_at(&self) -> Option<Timestamp> {
        self.checked_at
    }

    /// No complete poll for `OUTAGE_AFTER` in the current window.
    pub fn outage(&self, now: Timestamp) -> Option<Outage> {
        let (_, _, opened) = self.window?;
        let since = self
            .checked_at
            .map_or(opened, |checked| checked.max(opened));
        (now.duration_since(since) >= OUTAGE_AFTER).then_some(Outage {
            since,
            cause: self.failure.unwrap_or(Cause::Server),
        })
    }
}

/// A line of the way being watched, as named on screen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineStatus {
    pub line: String,
    pub mode: &'static str,
    /// "#RRGGBB", when configured.
    pub color: Option<String>,
    /// `blocking` or `disrupted`; `None` when fine.
    pub severity: Option<&'static str>,
}

/// What the poll loop last saw, shared with the API.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub polled_at: Option<Timestamp>,
    pub watching: Option<Direction>,
    pub health: Health,
    pub lines: Vec<LineStatus>,
    pub planned: Vec<String>,
}

/// The answer to `GET /status`, in the shape fixed by `fixtures/api/`.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct View {
    pub watching: Option<Direction>,
    pub checked_at: Option<Timestamp>,
    pub next: Option<Timestamp>,
    pub outage: Option<Outage>,
    pub lines: Vec<LineStatus>,
    pub planned: Vec<String>,
    /// The name of the school holidays today is in, if any, and their first and last days.
    pub school_holiday: Option<String>,
    pub school_holiday_first: Option<Date>,
    pub school_holiday_last: Option<Date>,
}

impl Snapshot {
    /// `next` is computed when asked, so that it follows a change of the days at
    /// once; `school_holiday` is as last read.
    pub fn view(
        &self,
        now: Timestamp,
        next: Option<Timestamp>,
        school_holiday: Option<Holiday>,
    ) -> View {
        // A poll loop that stopped cannot say so itself: the time of its last poll does.
        let stopped = self
            .polled_at
            .filter(|polled| now.duration_since(*polled) >= OUTAGE_AFTER)
            .map(|since| Outage {
                since,
                cause: Cause::Server,
            });
        View {
            watching: self.watching,
            checked_at: self.health.checked_at,
            next,
            outage: stopped.or_else(|| self.health.outage(now)),
            lines: self.lines.clone(),
            planned: self.planned.clone(),
            school_holiday_first: school_holiday.as_ref().map(|holiday| holiday.first),
            school_holiday_last: school_holiday.as_ref().map(|holiday| holiday.last),
            school_holiday: school_holiday.map(|holiday| holiday.name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture;
    use jiff::civil::date;

    const TUESDAY: Date = date(2026, 11, 3);

    /// A time on Tuesday 3 November 2026, in UTC.
    fn at(time: &str) -> Timestamp {
        format!("2026-11-03T{time}:00Z").parse().unwrap()
    }

    /// Watching the morning way from 6:00 UTC, checked at that time.
    fn morning() -> Health {
        let mut health = Health::default();
        health.watching(at("06:00"), TUESDAY, Direction::Outbound);
        health.checked(at("06:00"));
        health
    }

    fn outage(since: &str, cause: Cause) -> Option<Outage> {
        Some(Outage {
            since: at(since),
            cause,
        })
    }

    #[test]
    fn an_outage_starts_after_ten_minutes_without_a_complete_check() {
        let mut health = morning();
        assert!(!health.failed(at("06:02"), Cause::Prim));
        assert_eq!(health.outage(at("06:09")), None);
        assert!(health.failed(at("06:10"), Cause::Prim));
        assert_eq!(health.outage(at("06:10")), outage("06:00", Cause::Prim));
    }

    #[test]
    fn the_outage_is_notified_once_and_so_is_its_end() {
        let mut health = morning();
        assert!(health.failed(at("06:10"), Cause::Prim));
        health.outage_sent();
        assert!(!health.failed(at("06:12"), Cause::Prim));
        assert!(health.recovering());
        health.checked(at("06:14"));
        assert!(!health.recovering());
        assert_eq!(health.outage(at("06:14")), None);
    }

    #[test]
    fn an_outage_not_sent_is_due_again() {
        let mut health = morning();
        assert!(health.failed(at("06:10"), Cause::Firebase));
        assert!(health.failed(at("06:12"), Cause::Firebase));
        assert!(!health.recovering());
        assert_eq!(health.outage(at("06:12")), outage("06:00", Cause::Firebase));
    }

    #[test]
    fn a_window_has_ten_minutes_from_its_first_poll() {
        // Checked yesterday evening: the morning window still gets its ten minutes.
        let mut health = Health::default();
        health.checked("2026-11-02T16:58:00Z".parse().unwrap());
        health.watching(at("06:00"), TUESDAY, Direction::Outbound);
        assert_eq!(health.outage(at("06:09")), None);
        // Without a failure on record, the service itself is at fault (e.g. no device to notify).
        assert_eq!(health.outage(at("06:10")), outage("06:00", Cause::Server));
    }

    #[test]
    fn a_new_window_starts_afresh_but_still_owes_the_end_of_an_outage() {
        let mut health = morning();
        health.failed(at("06:10"), Cause::Prim);
        health.outage_sent();
        health.watching(at("15:00"), TUESDAY, Direction::Inbound);
        // "Interrupted" may have stayed on the phone all day: "restored" still follows.
        assert!(health.recovering());
        assert_eq!(health.outage(at("15:09")), None);
        assert_eq!(health.checked_at(), Some(at("06:00")));
        // Still out in this window: said again.
        assert!(health.failed(at("15:10"), Cause::Prim));
    }

    #[test]
    fn the_same_window_keeps_its_clock_even_after_a_restart() {
        let mut health = morning();
        health.failed(at("06:04"), Cause::Prim);
        let saved = serde_json::to_string(&health).unwrap();
        let mut restored: Health = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored, health);
        restored.watching(at("06:06"), TUESDAY, Direction::Outbound);
        assert!(restored.failed(at("06:10"), Cause::Prim));
    }

    #[test]
    fn outside_the_windows_nothing_is_out_but_the_end_of_an_outage_is_still_owed() {
        let mut health = morning();
        health.failed(at("06:10"), Cause::Prim);
        health.outage_sent();
        health.idle();
        assert_eq!(health.outage(at("09:00")), None);
        assert!(health.recovering());
        assert_eq!(health.checked_at(), Some(at("06:00")));
        health.checked(at("15:00"));
        assert!(!health.recovering());
    }

    fn line(
        name: &str,
        mode: &'static str,
        color: Option<&str>,
        severity: Option<&'static str>,
    ) -> LineStatus {
        LineStatus {
            line: name.into(),
            mode,
            color: color.map(String::from),
            severity,
        }
    }

    #[test]
    fn the_status_while_watching_matches_the_fixture() {
        let snapshot = Snapshot {
            polled_at: Some(at("06:42")),
            watching: Some(Direction::Outbound),
            health: Health {
                checked_at: Some(at("06:42")),
                ..morning()
            },
            lines: vec![
                line("L", "train", Some("#7B61C9"), Some("blocking")),
                line("X", "metro", Some("#E07A1F"), None),
            ],
            planned: vec!["Ligne L : travaux entre Nanterre et Cergy".into()],
        };
        // In made-up holidays, from Saturday to the next Sunday.
        let holiday = Holiday {
            name: "Vacances d'automne".into(),
            first: date(2026, 10, 31),
            last: date(2026, 11, 8),
        };
        assert_eq!(
            serde_json::to_value(snapshot.view(at("06:43"), Some(at("15:00")), Some(holiday)))
                .unwrap(),
            fixture("status-watching")
        );
    }

    #[test]
    fn the_status_during_an_outage_matches_the_fixture() {
        let mut health = Health::default();
        health.watching(at("15:00"), TUESDAY, Direction::Inbound);
        health.checked(at("15:20"));
        health.failed(at("15:22"), Cause::Prim);
        let snapshot = Snapshot {
            polled_at: Some(at("15:30")),
            watching: Some(Direction::Inbound),
            health,
            lines: vec![
                line("X", "metro", None, None),
                line("L", "train", Some("#7B61C9"), Some("disrupted")),
            ],
            planned: vec![],
        };
        assert_eq!(
            serde_json::to_value(snapshot.view(
                at("15:31"),
                Some("2026-11-05T05:45:00Z".parse().unwrap()),
                None
            ))
            .unwrap(),
            fixture("status-outage")
        );
    }

    #[test]
    fn the_status_outside_the_windows_matches_the_fixture() {
        let mut health = Health::default();
        health.checked(at("07:14"));
        let snapshot = Snapshot {
            polled_at: Some(at("10:00")),
            watching: None,
            health,
            lines: vec![],
            planned: vec![],
        };
        assert_eq!(
            serde_json::to_value(snapshot.view(at("10:01"), Some(at("15:00")), None)).unwrap(),
            fixture("status-idle")
        );
    }

    #[test]
    fn a_poll_loop_that_stopped_is_an_outage_even_outside_the_windows() {
        let snapshot = Snapshot {
            polled_at: Some(at("10:00")),
            ..Snapshot::default()
        };
        assert_eq!(snapshot.view(at("10:09"), None, None).outage, None);
        assert_eq!(
            snapshot.view(at("10:10"), None, None).outage,
            outage("10:00", Cause::Server)
        );
        // Before the first poll, there is nothing to say yet.
        assert_eq!(
            Snapshot::default().view(at("10:10"), None, None).outage,
            None
        );
    }
}
