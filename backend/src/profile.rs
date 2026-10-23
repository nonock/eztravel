//! What the phone sets through the API: which devices to notify, until
//! when the user is away, the days set one by one, the usual train of each
//! way, and the train picked today for the reminder to leave.
//! Shared by the API and the poll loop, saved in the data folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use jiff::civil::{Date, Time};
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

use crate::calendar::{Calendar, Day, Direction, LOOKAHEAD_DAYS};
use crate::service::{load, save};

/// Tokens change when the app is reinstalled: keep the latest few only.
pub const MAX_DEVICES: usize = 5;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Firebase tokens, oldest first.
    pub devices: Vec<String>,
    /// Last day away from the office, included.
    pub away_through: Option<Date>,
    /// The train to be reminded of, instead of the usual one.
    #[serde(default)]
    pub pick: Option<Pick>,
    #[serde(default)]
    pub usual: UsualTrains,
    /// Days set otherwise than planned, from today on.
    #[serde(default)]
    pub days: BTreeMap<Date, Day>,
}

/// Why a day cannot be set, in the shape fixed by `fixtures/api/day-refused.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DayRefusal {
    /// Before today.
    Past,
    /// Beyond the year the backend looks ahead.
    TooFar,
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

    /// The configured calendar, with the time away and the days set from the app.
    pub fn calendar(&self, configured: &Calendar) -> Calendar {
        Calendar {
            away_through: self.away_through,
            days: self.days.clone(),
            ..configured.clone()
        }
    }

    /// Sets `date` to `day`, or puts it back as planned with none. Only days
    /// that differ from the plan are kept, from `today` on.
    pub fn set_day(
        &mut self,
        configured: &Calendar,
        today: Date,
        date: Date,
        day: Option<Day>,
    ) -> Result<(), DayRefusal> {
        if date < today {
            return Err(DayRefusal::Past);
        }
        if date >= today + LOOKAHEAD_DAYS.days() {
            return Err(DayRefusal::TooFar);
        }
        self.days.retain(|kept, _| *kept >= today);
        self.days.remove(&date);
        if let Some(day) = day
            && self.calendar(configured).planned(date).0 != day.into()
        {
            self.days.insert(date, day);
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
        store.update(|profile| {
            profile.register("phone");
            profile.away_through = Some(date(2026, 12, 24));
            profile.pick = Some(pick.clone());
            profile.usual = usual;
            profile.days = days.clone();
        });
        // The seed only applies when nothing was saved.
        let reopened = Store::open(&path, &["other seed".into()]);
        assert_eq!(
            reopened.profile(),
            Profile {
                devices: vec!["seed".into(), "phone".into()],
                away_through: Some(date(2026, 12, 24)),
                pick: Some(pick),
                usual,
                days,
            }
        );
    }

    #[test]
    fn a_profile_saved_by_the_previous_version_is_read() {
        // Unreadable, it would give way to the seed: the phone registered since would be lost.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        std::fs::write(&path, r#"{"devices":["old","phone"],"away_through":null}"#).unwrap();
        let profile = Store::open(&path, &["seed".into()]).profile();
        assert_eq!(tokens(&profile), ["old", "phone"]);
        assert_eq!(profile.pick, None);
        assert_eq!(profile.usual, UsualTrains::default());
        assert_eq!(profile.days, BTreeMap::new());
    }

    #[test]
    fn the_calendar_is_the_configured_one_with_what_the_app_set() {
        let profile = Profile {
            away_through: Some(date(2026, 11, 3)),
            days: BTreeMap::from([(date(2026, 11, 5), Day::Remote)]),
            ..Profile::default()
        };
        assert_eq!(
            profile.calendar(&configured()),
            Calendar {
                away_through: Some(date(2026, 11, 3)),
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
            assert_eq!(
                profile.set_day(&configured, today, date(2026, 11, day), Some(set)),
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
        profile
            .set_day(&configured, today, date(2026, 11, 5), None)
            .unwrap();
        profile
            .set_day(&configured, today, date(2026, 11, 6), Some(Day::Remote))
            .unwrap();
        // Tuesday 3 is an office day, and away is the plan while away.
        profile
            .set_day(&configured, today, date(2026, 11, 3), Some(Day::Office))
            .unwrap();
        profile.away_through = Some(date(2026, 11, 13));
        profile
            .set_day(&configured, today, date(2026, 11, 12), Some(Day::Away))
            .unwrap();
        assert_eq!(
            profile.days,
            BTreeMap::from([(date(2026, 11, 11), Day::Office)])
        );
    }

    #[test]
    fn days_gone_by_are_refused_then_forgotten_and_so_are_days_too_far() {
        let configured = configured();
        let mut profile = Profile::default();
        profile
            .set_day(
                &configured,
                date(2026, 11, 2),
                date(2026, 11, 5),
                Some(Day::Remote),
            )
            .unwrap();
        // Today can still be set, the day before cannot.
        let today = date(2026, 11, 6);
        assert_eq!(
            profile.set_day(&configured, today, date(2026, 11, 5), None),
            Err(DayRefusal::Past)
        );
        assert_eq!(profile.days.len(), 1);
        assert_eq!(
            profile.set_day(&configured, today, today, Some(Day::Away)),
            Ok(())
        );
        // Each change forgets the days gone by.
        assert_eq!(profile.days, BTreeMap::from([(today, Day::Away)]));
        // The backend looks a year ahead, no further.
        let last = today + (LOOKAHEAD_DAYS - 1).days();
        assert_eq!(
            profile.set_day(&configured, today, last, Some(Day::Away)),
            Ok(())
        );
        assert_eq!(
            profile.set_day(&configured, today, last.tomorrow().unwrap(), None),
            Err(DayRefusal::TooFar)
        );
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
