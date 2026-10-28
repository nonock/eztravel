//! What the phone sets through the API: which devices to notify, the days
//! set one by one, the usual train of each way, and the train picked today
//! for the reminder to leave.
//! Shared by the API and the poll loop, saved in the data folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use jiff::civil::{Date, Time};
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

use crate::calendar::{Calendar, Day, Direction, LOOKAHEAD_DAYS, WatchWindows};
use crate::service::{load, save};

/// Tokens change when the app is reinstalled: keep the latest few only.
pub const MAX_DEVICES: usize = 5;

/// Read leaving out keys this version no longer knows, as the first versions' time away.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Firebase tokens, oldest first.
    pub devices: Vec<String>,
    /// The train to be reminded of, instead of the usual one.
    #[serde(default)]
    pub pick: Option<Pick>,
    #[serde(default)]
    pub usual: UsualTrains,
    /// Days set otherwise than planned, from today on.
    #[serde(default)]
    pub days: BTreeMap<Date, Day>,
    /// The way home of a day, set from the app: it wins over the rules.
    #[serde(default)]
    pub late: Option<LateDay>,
}

/// The way home of `day`, set from the app: late, leaving at `leave`, or as
/// usual with none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LateDay {
    pub day: Date,
    pub leave: Option<Time>,
}

/// A late way home, in the shape `GET /status` gives it: when it leaves and
/// until when it is watched, local, and whether it was set from the app
/// rather than by the rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct LateView {
    pub leave: Time,
    pub until: Time,
    pub set: bool,
}

/// Why a day cannot be set, in the shape fixed by `fixtures/api/day-refused.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DayRefusal {
    /// Before today.
    Past,
    /// Beyond the year the backend looks ahead.
    TooFar,
    /// Through a day before the first.
    Backwards,
}

/// The usual train of each way, by its time at the station of the ride the
/// reminder is for: on office days, the first train from then is reminded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsualTrains {
    pub outbound: Option<Time>,
    pub inbound: Option<Time>,
}

impl UsualTrains {
    pub fn of(&self, direction: Direction) -> Option<Time> {
        match direction {
            Direction::Outbound => self.outbound,
            Direction::Inbound => self.inbound,
        }
    }

    pub fn set(&mut self, direction: Direction, time: Option<Time>) {
        match direction {
            Direction::Outbound => self.outbound = time,
            Direction::Inbound => self.inbound = time,
        }
    }
}

/// A train picked in the app, on its day and way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    pub day: Date,
    pub direction: Direction,
    /// The train, by its journey.
    pub journey: String,
    /// When to leave for it, as it was when picked. Should it be cancelled,
    /// the first train that can be caught leaving from then takes its place.
    pub leave: Timestamp,
}

impl Profile {
    /// Adds a device, or makes it the most recent if already known.
    pub fn register(&mut self, token: &str) {
        self.forget(token);
        self.devices.push(token.to_string());
        let extra = self.devices.len().saturating_sub(MAX_DEVICES);
        self.devices.drain(..extra);
    }

    pub fn forget(&mut self, token: &str) {
        self.devices.retain(|known| known != token);
    }

    /// When the way home leaves on `day`, if it is late: as set from the app
    /// for that day, or else by the rules.
    pub fn late_on(&self, day: Date, configured: &WatchWindows) -> Option<Time> {
        match self.late {
            Some(late) if late.day == day => late.leave,
            _ => configured.late_on(day),
        }
    }

    /// The watch windows of `day`, with its way home.
    pub fn windows(&self, configured: &WatchWindows, day: Date) -> WatchWindows {
        configured.with_late(self.late_on(day, configured))
    }

    /// The way home of `day`, if it is late.
    pub fn late(&self, day: Date, configured: &WatchWindows) -> Option<LateView> {
        let leave = self.late_on(day, configured)?;
        Some(LateView {
            leave,
            until: configured.with_late(Some(leave)).inbound.end,
            set: self.late.is_some_and(|late| late.day == day),
        })
    }

    /// The usual trains of `day`: none home when it is late.
    pub fn usual_on(&self, day: Date, configured: &WatchWindows) -> UsualTrains {
        let mut usual = self.usual;
        if self.late_on(day, configured).is_some() {
            usual.inbound = None;
        }
        usual
    }

    /// The configured calendar, with the days set from the app.
    pub fn calendar(&self, configured: &Calendar) -> Calendar {
        Calendar {
            days: self.days.clone(),
            ..configured.clone()
        }
    }

    /// Sets each day from `first` through `last` to `day`, or puts them back as
    /// planned with none, all or none of them. Only days that differ from the
    /// plan are kept, from `today` on.
    pub fn set_days(
        &mut self,
        configured: &Calendar,
        today: Date,
        first: Date,
        last: Date,
        day: Option<Day>,
    ) -> Result<(), DayRefusal> {
        if first < today {
            return Err(DayRefusal::Past);
        }
        if last >= today + LOOKAHEAD_DAYS.days() {
            return Err(DayRefusal::TooFar);
        }
        if last < first {
            return Err(DayRefusal::Backwards);
        }
        self.days.retain(|kept, _| *kept >= today);
        let calendar = self.calendar(configured);
        for date in first.series(1.day()).take_while(|date| *date <= last) {
            self.days.remove(&date);
            if let Some(day) = day
                && calendar.planned(date).0 != day.into()
            {
                self.days.insert(date, day);
            }
        }
        Ok(())
    }
}

/// The profile behind a lock, saved after each change.
pub struct Store {
    path: PathBuf,
    profile: Mutex<Profile>,
}

impl Store {
    /// Picks up the saved profile; without one, starts with `seed_devices`.
    pub fn open(path: &Path, seed_devices: &[String]) -> Self {
        let profile = load(path).unwrap_or_else(|| Profile {
            devices: seed_devices.to_vec(),
            ..Profile::default()
        });
        Self {
            path: path.to_path_buf(),
            profile: Mutex::new(profile),
        }
    }

    /// A copy, so that no lock is held while the caller works.
    pub fn profile(&self) -> Profile {
        self.profile
            .lock()
            .expect("no thread panics while holding the profile")
            .clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Profile)) {
        let mut profile = self
            .profile
            .lock()
            .expect("no thread panics while holding the profile");
        change(&mut profile);
        save(&self.path, &*profile);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{Calendar, Day};
    use jiff::ToSpan;
    use jiff::civil::{date, time};

    #[test]
    fn a_way_home_set_for_its_day_wins_over_the_rules() {
        let configured: crate::calendar::WatchWindows = toml::from_str(
            r#"
            outbound = { start = "06:45", end = "08:15" }
            inbound = { start = "16:00", end = "17:30" }
            late = [{ first = "2026-11-04", every_weeks = 2, leave = "20:00" }]
            "#,
        )
        .unwrap();
        let wednesday = date(2026, 11, 4);
        let mut profile = Profile::default();
        assert_eq!(
            profile.late_on(wednesday, &configured),
            Some(time(20, 0, 0, 0))
        );
        assert_eq!(
            profile.late(wednesday, &configured),
            Some(LateView {
                leave: time(20, 0, 0, 0),
                until: time(21, 0, 0, 0),
                set: false
            })
        );
        // Later than the rule says, that day.
        profile.late = Some(LateDay {
            day: wednesday,
            leave: Some(time(22, 0, 0, 0)),
        });
        assert_eq!(
            profile.windows(&configured, wednesday).inbound.end,
            time(23, 0, 0, 0)
        );
        assert_eq!(
            profile.late(wednesday, &configured),
            Some(LateView {
                leave: time(22, 0, 0, 0),
                until: time(23, 0, 0, 0),
                set: true
            })
        );
        // The usual train home is not that day's.
        profile
            .usual
            .set(Direction::Outbound, Some(time(8, 0, 0, 0)));
        profile
            .usual
            .set(Direction::Inbound, Some(time(17, 0, 0, 0)));
        assert_eq!(
            profile.usual_on(wednesday, &configured),
            UsualTrains {
                outbound: Some(time(8, 0, 0, 0)),
                inbound: None
            }
        );
        assert_eq!(
            profile.usual_on(date(2026, 11, 5), &configured),
            profile.usual
        );
        // As usual that day, whatever the rule.
        profile.late = Some(LateDay {
            day: wednesday,
            leave: None,
        });
        assert_eq!(profile.late_on(wednesday, &configured), None);
        assert_eq!(profile.late(wednesday, &configured), None);
        assert_eq!(profile.windows(&configured, wednesday), configured);
        // Only that day: two weeks on, the rule again.
        assert_eq!(
            profile.late_on(date(2026, 11, 18), &configured),
            Some(time(20, 0, 0, 0))
        );
    }

    /// The example's calendar: Tuesday and Thursday the week of 2 November 2026,
    /// Monday, Wednesday and Friday the next.
    fn configured() -> Calendar {
        crate::config::Config::parse(include_str!("../config.example.toml"))
            .unwrap()
            .calendar
    }

    fn tokens(profile: &Profile) -> Vec<&str> {
        profile.devices.iter().map(String::as_str).collect()
    }

    #[test]
    fn a_device_is_registered_once_and_the_latest_comes_last() {
        let mut profile = Profile::default();
        profile.register("a");
        profile.register("b");
        profile.register("a");
        assert_eq!(tokens(&profile), ["b", "a"]);
    }

    #[test]
    fn only_the_latest_devices_are_kept() {
        let mut profile = Profile::default();
        for token in ["1", "2", "3", "4", "5", "6", "7"] {
            profile.register(token);
        }
        assert_eq!(tokens(&profile), ["3", "4", "5", "6", "7"]);
    }

    #[test]
    fn a_device_can_be_forgotten() {
        let mut profile = Profile {
            devices: vec!["a".into(), "b".into()],
            ..Profile::default()
        };
        profile.forget("a");
        profile.forget("unknown");
        assert_eq!(tokens(&profile), ["b"]);
    }

    #[test]
    fn the_store_saves_each_change_and_finds_it_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let store = Store::open(&path, &["seed".into()]);
        assert_eq!(tokens(&store.profile()), ["seed"]);
        let pick = Pick {
            day: date(2026, 11, 3),
            direction: Direction::Inbound,
            journey: "j1".into(),
            leave: "2026-11-03T17:05:00Z".parse().unwrap(),
        };
        let usual = UsualTrains {
            outbound: Some(time(8, 7, 0, 0)),
            inbound: None,
        };
        let days = BTreeMap::from([(date(2026, 12, 23), Day::Remote)]);
        let late = LateDay {
            day: date(2026, 12, 23),
            leave: Some(time(21, 0, 0, 0)),
        };
        store.update(|profile| {
            profile.register("phone");
            profile.pick = Some(pick.clone());
            profile.usual = usual;
            profile.days = days.clone();
            profile.late = Some(late);
        });
        // The seed only applies when nothing was saved.
        let reopened = Store::open(&path, &["other seed".into()]);
        assert_eq!(
            reopened.profile(),
            Profile {
                devices: vec!["seed".into(), "phone".into()],
                pick: Some(pick),
                usual,
                days,
                late: Some(late),
            }
        );
    }

    #[test]
    fn a_profile_saved_by_the_previous_version_is_read() {
        // Unreadable, it would give way to the seed: the phone registered since would be lost.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        // The first version kept the time away, since given up for days set one by one.
        std::fs::write(
            &path,
            r#"{"devices":["old","phone"],"away_through":"2026-12-24"}"#,
        )
        .unwrap();
        let profile = Store::open(&path, &["seed".into()]).profile();
        assert_eq!(tokens(&profile), ["old", "phone"]);
        assert_eq!(profile.pick, None);
        assert_eq!(profile.usual, UsualTrains::default());
        assert_eq!(profile.days, BTreeMap::new());
    }

    #[test]
    fn the_calendar_is_the_configured_one_with_what_the_app_set() {
        let profile = Profile {
            days: BTreeMap::from([(date(2026, 11, 5), Day::Remote)]),
            ..Profile::default()
        };
        assert_eq!(
            profile.calendar(&configured()),
            Calendar {
                days: BTreeMap::from([(date(2026, 11, 5), Day::Remote)]),
                ..configured()
            }
        );
    }

    #[test]
    fn a_day_is_set_or_put_back_as_planned() {
        let (configured, today) = (configured(), date(2026, 11, 2));
        let mut profile = Profile::default();
        // Thursday 5 at home, Friday 6 away, Wednesday 11 (a holiday) at the office.
        for (day, set) in [(5, Day::Remote), (6, Day::Away), (11, Day::Office)] {
            let day = date(2026, 11, day);
            assert_eq!(
                profile.set_days(&configured, today, day, day, Some(set)),
                Ok(())
            );
        }
        assert_eq!(
            profile.days,
            BTreeMap::from([
                (date(2026, 11, 5), Day::Remote),
                (date(2026, 11, 6), Day::Away),
                (date(2026, 11, 11), Day::Office),
            ])
        );
        // Put back as planned with none, or by setting what it would be anyway.
        let (fifth, sixth) = (date(2026, 11, 5), date(2026, 11, 6));
        profile
            .set_days(&configured, today, fifth, fifth, None)
            .unwrap();
        profile
            .set_days(&configured, today, sixth, sixth, Some(Day::Remote))
            .unwrap();
        // Tuesday 3 is an office day.
        let third = date(2026, 11, 3);
        profile
            .set_days(&configured, today, third, third, Some(Day::Office))
            .unwrap();
        assert_eq!(
            profile.days,
            BTreeMap::from([(date(2026, 11, 11), Day::Office)])
        );
    }

    #[test]
    fn days_off_are_set_from_one_day_through_another() {
        let (configured, today) = (configured(), date(2026, 11, 2));
        let mut profile = Profile::default();
        // From Thursday 5 through Monday 9, the weekend included.
        assert_eq!(
            profile.set_days(
                &configured,
                today,
                date(2026, 11, 5),
                date(2026, 11, 9),
                Some(Day::Away)
            ),
            Ok(())
        );
        let away =
            |days: std::ops::RangeInclusive<i8>| days.map(|day| (date(2026, 11, day), Day::Away));
        assert_eq!(profile.days, away(5..=9).collect());
        // Put back as planned from Saturday 7 on: the rest stays.
        profile
            .set_days(
                &configured,
                today,
                date(2026, 11, 7),
                date(2026, 11, 9),
                None,
            )
            .unwrap();
        assert_eq!(profile.days, away(5..=6).collect());
    }

    #[test]
    fn days_gone_by_are_refused_then_forgotten_and_so_are_days_too_far() {
        let configured = configured();
        let mut profile = Profile::default();
        let fifth = date(2026, 11, 5);
        profile
            .set_days(
                &configured,
                date(2026, 11, 2),
                fifth,
                fifth,
                Some(Day::Remote),
            )
            .unwrap();
        // Today can still be set, the day before cannot.
        let today = date(2026, 11, 6);
        assert_eq!(
            profile.set_days(&configured, today, fifth, today, None),
            Err(DayRefusal::Past)
        );
        assert_eq!(profile.days.len(), 1);
        assert_eq!(
            profile.set_days(&configured, today, today, today, Some(Day::Away)),
            Ok(())
        );
        // Each change forgets the days gone by.
        assert_eq!(profile.days, BTreeMap::from([(today, Day::Away)]));
        // The backend looks a year ahead, no further: refused as a whole.
        let last = today + (LOOKAHEAD_DAYS - 1).days();
        assert_eq!(
            profile.set_days(&configured, today, last, last, Some(Day::Away)),
            Ok(())
        );
        let beyond = last.tomorrow().unwrap();
        for (from, through) in [(beyond, beyond), (last, beyond)] {
            assert_eq!(
                profile.set_days(&configured, today, from, through, None),
                Err(DayRefusal::TooFar)
            );
        }
        assert_eq!(profile.days.len(), 2);
    }

    #[test]
    fn days_through_one_before_the_first_are_refused() {
        let (configured, today) = (configured(), date(2026, 11, 2));
        let mut profile = Profile::default();
        assert_eq!(
            profile.set_days(
                &configured,
                today,
                date(2026, 11, 9),
                date(2026, 11, 5),
                Some(Day::Away)
            ),
            Err(DayRefusal::Backwards)
        );
        assert_eq!(profile.days, BTreeMap::new());
    }

    #[test]
    fn each_way_has_its_usual_train() {
        let mut usual = UsualTrains::default();
        usual.set(Direction::Inbound, Some(time(17, 52, 0, 0)));
        assert_eq!(usual.of(Direction::Inbound), Some(time(17, 52, 0, 0)));
        assert_eq!(usual.of(Direction::Outbound), None);
        usual.set(Direction::Outbound, Some(time(8, 7, 0, 0)));
        usual.set(Direction::Inbound, None);
        assert_eq!(
            usual,
            UsualTrains {
                outbound: Some(time(8, 7, 0, 0)),
                inbound: None
            }
        );
    }
}
