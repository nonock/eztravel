//! Trains of the watched way that do not run.
//!
//! PRIM seldom says that a train is cancelled: most often the train just
//! leaves the stop's list, half an hour or more before its time, and stays
//! out of it. A train missing for four polls in a row, while it cannot have
//! left yet, is taken as cancelled; four polls let a train that is only
//! being changed come back first. A train said "cancelled" is notified at
//! once, and "back" if it runs after all: that status is sometimes lifted.
//!
//! Only trains due to leave during the window are followed, from its
//! `trains_from`: those cancelled early are notified when the window opens.
//! Each train ends half an hour after its time, or with its window: how it
//! went is then kept for the statistics.
//!
//! A train cancelled well ahead may never be listed at all. The timetable of
//! the window is read once, so that a train it gives is missing too when
//! later trains are listed but never it.

use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::alerts::Notification;
use crate::calendar::Direction;
use crate::departures::{CallStatus, Visit, serves};

/// Polls in a row without a train before it is taken as cancelled.
pub const CONFIRM: u32 = 4;
/// A train due this soon may have left: its absence then says nothing.
const LEAVING: SignedDuration = SignedDuration::from_mins(2);
/// A train is forgotten this long after it was due.
const FORGET_AFTER: SignedDuration = SignedDuration::from_mins(30);

/// A train, as notified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Train {
    /// Line code ("C01740").
    pub line: String,
    pub mission: Option<String>,
    pub destination: String,
    /// The stop it leaves from, by name.
    pub from: String,
    /// Its departure in the timetable: with the line, what names it.
    pub aimed: Timestamp,
    /// Its departure as last listed: later than `aimed` when it runs late.
    pub expected: Timestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// It left the stop's list before its time.
    Missing,
    /// Due by the timetable, it never showed in the stop's list while later trains did.
    Unlisted,
    /// The operator says so.
    Announced,
}

/// How a followed train went, at its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Listed until it could have left.
    Ran,
    /// Out of the list four polls in a row before its time.
    Missing,
    /// Said cancelled by the operator, last it was listed.
    Announced,
}

/// A train followed, as it ended: for the statistics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ended {
    pub day: Date,
    pub direction: Direction,
    /// Its `expected` time is the last one listed.
    #[serde(flatten)]
    pub train: Train,
    pub outcome: Outcome,
    /// Notified as cancelled, then listed as running again.
    pub came_back: bool,
}

/// What a train leg's stops list at one poll.
pub struct Seen {
    pub line: String,
    /// The board stop's name.
    pub from: String,
    /// The alight stop's `MonitoringRef`.
    pub alight: String,
    pub board: Vec<Visit>,
    pub arriving: Vec<Visit>,
}

/// A train of the timetable, due to leave during the window on a train leg.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scheduled {
    pub line: String,
    /// The stop it leaves from, by name, as the live list names it.
    pub from: String,
    pub aimed: Timestamp,
    pub mission: Option<String>,
    pub destination: String,
}

/// The trains followed in the current window. Kept between polls, and across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trains {
    window: Option<(Date, Direction)>,
    /// The window whose timetable was read.
    #[serde(default)]
    timetable: Option<(Date, Direction)>,
    followed: Vec<Followed>,
    /// The trains that ended since this was last taken: whoever keeps them takes them.
    #[serde(skip)]
    pub ended: Vec<Ended>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Followed {
    journey: String,
    train: Train,
    /// Polls in a row without it, while it could not have left.
    missing: u32,
    said_cancelled: bool,
    notified: bool,
    #[serde(default)]
    came_back: bool,
    /// Listed live at least once; else only in the timetable.
    #[serde(default = "listed")]
    listed: bool,
}

fn listed() -> bool {
    true
}

impl Trains {
    /// Whether the timetable of the window `key` is still to be read.
    pub fn needs_timetable(&self, key: (Date, Direction)) -> bool {
        self.timetable != Some(key)
    }

    /// The window `key` begins: the trains of the one before end.
    fn open(&mut self, key: (Date, Direction)) {
        if self.window == Some(key) {
            return;
        }
        if let Some(previous) = self.window {
            let ended: Vec<Ended> = self
                .followed
                .drain(..)
                .filter_map(|followed| followed.end(previous))
                .collect();
            self.ended.extend(ended);
        }
        self.window = Some(key);
    }
}

/// The timetable of the window `key`: each train waits to be listed live.
pub fn plan(trains: &mut Trains, key: (Date, Direction), scheduled: &[Scheduled]) {
    trains.open(key);
    trains.timetable = Some(key);
    for train in scheduled {
        trains.followed.push(Followed {
            journey: format!("timetable:{}:{}", train.line, train.aimed),
            train: Train {
                line: train.line.clone(),
                mission: train.mission.clone(),
                destination: train.destination.clone(),
                from: train.from.clone(),
                aimed: train.aimed,
                expected: train.aimed,
            },
            missing: 0,
            said_cancelled: false,
            notified: false,
            came_back: false,
            listed: false,
        });
    }
}

/// A live train and one of the timetable are the same within this.
const SAME_TIME: SignedDuration = SignedDuration::from_secs(90);

impl Followed {
    /// How it went; nothing for a train of the timetable never listed nor confirmed missing.
    fn end(self, (day, direction): (Date, Direction)) -> Option<Ended> {
        if !self.listed && !self.notified && self.missing < CONFIRM {
            return None;
        }
        let outcome = if self.said_cancelled {
            Outcome::Announced
        } else if self.notified || self.missing >= CONFIRM {
            Outcome::Missing
        } else {
            Outcome::Ran
        };
        Some(Ended {
            day,
            direction,
            train: self.train,
            outcome,
            came_back: self.came_back,
        })
    }
}

/// Follows the trains of one poll. `window` is when the window opens and
/// ends; nothing is notified while `notify` is false, before it opens.
pub fn follow(
    trains: &mut Trains,
    key: (Date, Direction),
    window: (Timestamp, Timestamp),
    now: Timestamp,
    seen: &[Seen],
    notify: bool,
) -> Vec<Notification> {
    trains.open(key);
    let mut notifications = vec![];
    let mut listed: Vec<&str> = vec![];
    for leg in seen {
        for visit in leg.board.iter().filter(|visit| visit.line == leg.line) {
            // A call with no departure ends the journey here: nothing to board.
            let Some(expected) = visit.expected_departure.or(visit.aimed_departure) else {
                continue;
            };
            listed.push(&visit.journey);
            let said_cancelled = visit.status == CallStatus::Cancelled;
            let aimed = visit.aimed_departure.unwrap_or(expected);
            // Followed already, or waiting in the timetable: the nearest in time.
            let known = trains
                .followed
                .iter()
                .position(|followed| followed.journey == visit.journey)
                .or_else(|| {
                    trains
                        .followed
                        .iter()
                        .enumerate()
                        .filter(|(_, followed)| !followed.listed && followed.train.line == leg.line)
                        .map(|(index, followed)| {
                            (index, followed.train.aimed.duration_since(aimed).abs())
                        })
                        .filter(|(_, apart)| *apart <= SAME_TIME)
                        .min_by_key(|(_, apart)| *apart)
                        .map(|(index, _)| index)
                });
            if let Some(followed) = known.map(|index| &mut trains.followed[index]) {
                followed.journey = visit.journey.clone();
                followed.listed = true;
                followed.train.aimed = aimed;
                // A train being changed may come back under another mission.
                followed.train.mission = visit.mission.clone();
                followed.train.destination = visit.destination.clone();
                followed.train.expected = expected;
                followed.missing = 0;
                followed.said_cancelled = said_cancelled;
                if followed.notified && !said_cancelled {
                    followed.notified = false;
                    followed.came_back = true;
                    notifications.push(Notification::TrainBack {
                        train: followed.train.clone(),
                    });
                }
                continue;
            }
            let in_window = window.0 <= aimed && aimed < window.1;
            // Long gone, though still listed: it ended already.
            let gone = now.duration_since(expected) >= FORGET_AFTER;
            if !in_window || gone || serves(visit, expected, (&leg.alight, &leg.arriving)).is_none()
            {
                continue;
            }
            trains.followed.push(Followed {
                journey: visit.journey.clone(),
                train: Train {
                    line: leg.line.clone(),
                    mission: visit.mission.clone(),
                    destination: visit.destination.clone(),
                    from: leg.from.clone(),
                    aimed,
                    expected,
                },
                missing: 0,
                said_cancelled,
                notified: false,
                came_back: false,
                listed: true,
            });
        }
    }
    // A train of the timetable is missing only once later ones of its line are listed.
    let last_listed = |line: &str| {
        seen.iter()
            .filter(|leg| leg.line == line)
            .flat_map(|leg| &leg.board)
            .filter(|visit| visit.line == line)
            .filter_map(|visit| visit.aimed_departure)
            .max()
    };
    for followed in &mut trains.followed {
        let due = followed.listed
            || last_listed(&followed.train.line)
                .is_some_and(|last| last > followed.train.aimed + SAME_TIME);
        if due
            && !listed.contains(&followed.journey.as_str())
            && followed.train.expected.duration_since(now) > LEAVING
        {
            followed.missing += 1;
        }
        if !notify || followed.notified {
            continue;
        }
        let why = if followed.said_cancelled {
            Why::Announced
        } else if followed.missing >= CONFIRM {
            if followed.listed {
                Why::Missing
            } else {
                Why::Unlisted
            }
        } else {
            continue;
        };
        followed.notified = true;
        notifications.push(Notification::TrainCancelled {
            train: followed.train.clone(),
            why,
        });
    }
    let (kept, forgotten) = std::mem::take(&mut trains.followed)
        .into_iter()
        .partition(|followed| now.duration_since(followed.train.expected) < FORGET_AFTER);
    trains.followed = kept;
    let forgotten: Vec<Followed> = forgotten;
    trains.ended.extend(
        forgotten
            .into_iter()
            .filter_map(|followed| followed.end(key)),
    );
    notifications
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::departures::parse;
    use jiff::civil::date;
    use serde_json::{Value, json};

    const TUESDAY: Date = date(2026, 11, 3);
    const KEY: (Date, Direction) = (TUESDAY, Direction::Outbound);

    /// A time on Tuesday 3 November 2026, in UTC.
    fn at(time: &str) -> Timestamp {
        format!("2026-11-03T{time}:00Z").parse().unwrap()
    }

    /// The window of the example: 6:45 to 8:15 in Paris.
    fn window() -> (Timestamp, Timestamp) {
        (at("05:45"), at("07:15"))
    }

    /// A train of line L leaving the board stop at `aimed`, towards `destination`.
    fn visit(journey: &str, aimed: &str, expected: &str, status: &str, destination: u32) -> Value {
        json!({
            "RecordedAtTime": "2026-11-03T04:00:00Z",
            "MonitoredVehicleJourney": {
                "LineRef": { "value": "STIF:Line::C01740:" },
                "FramedVehicleJourneyRef": { "DatedVehicleJourneyRef": journey },
                "DestinationRef": { "value": format!("STIF:StopArea:SP:{destination}:") },
                "DestinationName": [{ "value": "Gare Centrale" }],
                "JourneyNote": [{ "value": "PALO" }],
                "MonitoredCall": {
                    "AimedDepartureTime": at(aimed).to_string(),
                    "ExpectedDepartureTime": at(expected).to_string(),
                    "DepartureStatus": status
                }
            }
        })
    }

    fn visits(list: Vec<Value>) -> Vec<Visit> {
        let answer = json!({ "Siri": { "ServiceDelivery": { "StopMonitoringDelivery": [{ "MonitoredStopVisit": list }] } } });
        parse(&serde_json::to_vec(&answer).unwrap()).unwrap()
    }

    /// The board stop lists `board`; the alight stop (SP:200) lists nothing.
    fn seen(board: Vec<Value>) -> Vec<Seen> {
        vec![Seen {
            line: "C01740".into(),
            from: "Les Aubépines".into(),
            alight: "STIF:StopArea:SP:200:".into(),
            board: visits(board),
            arriving: vec![],
        }]
    }

    /// The train j1 of 6:52 UTC, ending at the alight stop.
    fn j1(status: &str) -> Value {
        visit("j1", "06:52", "06:52", status, 200)
    }

    fn train(aimed: &str) -> Train {
        Train {
            line: "C01740".into(),
            mission: Some("PALO".into()),
            destination: "Gare Centrale".into(),
            from: "Les Aubépines".into(),
            aimed: at(aimed),
            expected: at(aimed),
        }
    }

    fn cancelled(why: Why) -> Vec<Notification> {
        vec![Notification::TrainCancelled {
            train: train("06:52"),
            why,
        }]
    }

    /// Polls every two minutes from `from` with what `board` gives, collecting notifications.
    fn polls(
        trains: &mut Trains,
        from: &str,
        count: usize,
        notify: bool,
        board: impl Fn() -> Vec<Value>,
    ) -> Vec<Notification> {
        let start = at(from);
        (0..count)
            .flat_map(|n| {
                let now = start + SignedDuration::from_mins(2 * n as i64);
                follow(trains, KEY, window(), now, &seen(board()), notify)
            })
            .collect()
    }

    /// A train of line L in the timetable, due to leave the board stop at `aimed`.
    fn scheduled(aimed: &str) -> Scheduled {
        Scheduled {
            line: "C01740".into(),
            from: "Les Aubépines".into(),
            aimed: at(aimed),
            mission: Some("PALO".into()),
            destination: "Gare Centrale".into(),
        }
    }

    #[test]
    fn a_train_of_the_timetable_never_listed_is_cancelled_once_later_ones_are() {
        let mut trains = Trains::default();
        assert!(trains.needs_timetable(KEY));
        plan(&mut trains, KEY, &[scheduled("06:52"), scheduled("07:02")]);
        assert!(!trains.needs_timetable(KEY));
        assert!(trains.needs_timetable((TUESDAY, Direction::Inbound)));
        // Only the 7:02 shows: the 6:52 never does, though it would come first.
        let later = || vec![visit("j2", "07:02", "07:02", "onTime", 200)];
        assert_eq!(polls(&mut trains, "05:50", 3, true, later), []);
        assert_eq!(
            polls(&mut trains, "05:56", 1, true, later),
            cancelled(Why::Unlisted)
        );
        assert_eq!(polls(&mut trains, "05:58", 1, true, later), []);
        // Kept as missing once forgotten; the 7:02 ran.
        polls(&mut trains, "07:40", 1, true, Vec::new);
        let ended: Vec<_> = trains
            .ended
            .iter()
            .map(|ended| (ended.train.aimed, ended.outcome))
            .collect();
        assert_eq!(
            ended,
            [(at("06:52"), Outcome::Missing), (at("07:02"), Outcome::Ran)]
        );
    }

    #[test]
    fn a_listed_train_takes_its_place_in_the_timetable() {
        let mut trains = Trains::default();
        plan(&mut trains, KEY, &[scheduled("06:52")]);
        assert_eq!(
            polls(&mut trains, "05:50", 1, true, || vec![j1("onTime")]),
            []
        );
        // One train, not two.
        assert_eq!(trains.followed.len(), 1);
        assert_eq!(trains.followed[0].journey, "j1");
        // Then gone from the list: missing the usual way.
        assert_eq!(polls(&mut trains, "05:52", 3, true, Vec::new), []);
        assert_eq!(
            polls(&mut trains, "05:58", 1, true, Vec::new),
            cancelled(Why::Missing)
        );
    }

    #[test]
    fn a_train_of_the_timetable_beyond_the_list_is_not_missing() {
        let mut trains = Trains::default();
        plan(&mut trains, KEY, &[scheduled("07:10")]);
        // The list does not reach it yet.
        assert_eq!(
            polls(&mut trains, "05:50", 6, true, || vec![j1("onTime")]),
            []
        );
        // A minute and a half apart, under another mission, it is the same train.
        let renamed = || {
            let mut later = visit("j9", "07:11", "07:11", "onTime", 200);
            later["MonitoredVehicleJourney"]["JourneyNote"] = json!([{ "value": "PILO" }]);
            vec![j1("onTime"), later]
        };
        assert_eq!(polls(&mut trains, "06:02", 6, true, renamed), []);
        let journeys: Vec<&str> = trains
            .followed
            .iter()
            .map(|followed| followed.journey.as_str())
            .collect();
        assert_eq!(journeys, ["j9", "j1"]);
    }

    #[test]
    fn a_train_of_the_timetable_never_confirmed_is_not_kept() {
        let mut trains = Trains::default();
        plan(&mut trains, KEY, &[scheduled("07:10")]);
        polls(&mut trains, "05:50", 1, true, || vec![j1("onTime")]);
        // The next window ends this one: of the 7:10, nothing is known.
        follow(
            &mut trains,
            (TUESDAY, Direction::Inbound),
            window(),
            at("15:00"),
            &[],
            false,
        );
        let ended: Vec<_> = trains.ended.iter().map(|ended| ended.train.aimed).collect();
        assert_eq!(ended, [at("06:52")]);
    }

    #[test]
    fn a_train_saved_by_the_previous_version_was_listed() {
        let saved = r#"{"window":["2026-11-03","outbound"],"followed":[{"journey":"j1","train":{"line":"C01740","mission":"PALO","destination":"Gare Centrale","from":"Les Aubépines","aimed":"2026-11-03T06:52:00Z","expected":"2026-11-03T06:52:00Z"},"missing":0,"said_cancelled":false,"notified":false}]}"#;
        let mut trains: Trains = serde_json::from_str(saved).unwrap();
        assert!(trains.needs_timetable(KEY));
        assert_eq!(polls(&mut trains, "05:50", 3, true, Vec::new), []);
        assert_eq!(
            polls(&mut trains, "05:56", 1, true, Vec::new),
            cancelled(Why::Missing)
        );
    }

    #[test]
    fn a_train_missing_four_polls_in_a_row_is_cancelled() {
        let mut trains = Trains::default();
        assert_eq!(
            polls(&mut trains, "05:50", 1, true, || vec![j1("onTime")]),
            []
        );
        assert_eq!(polls(&mut trains, "05:52", 3, true, Vec::new), []);
        assert_eq!(
            polls(&mut trains, "05:58", 1, true, Vec::new),
            cancelled(Why::Missing)
        );
        // Said once.
        assert_eq!(polls(&mut trains, "06:00", 5, true, Vec::new), []);
    }

    #[test]
    fn a_train_back_before_the_fourth_poll_is_not_cancelled() {
        // As a train being changed: it leaves the list, then comes back under a new mission.
        let mut trains = Trains::default();
        polls(&mut trains, "05:50", 1, true, || vec![j1("onTime")]);
        assert_eq!(polls(&mut trains, "05:52", 3, true, Vec::new), []);
        assert_eq!(
            polls(&mut trains, "05:58", 1, true, || vec![j1("onTime")]),
            []
        );
        // Counted again from the start.
        assert_eq!(polls(&mut trains, "06:00", 3, true, Vec::new), []);
    }

    #[test]
    fn a_train_that_may_have_left_is_not_missing() {
        let mut trains = Trains::default();
        polls(&mut trains, "06:44", 1, true, || vec![j1("onTime")]);
        // Missing at 6:46 and 6:48; from 6:50, it may be gone: it was due at 6:52.
        assert_eq!(polls(&mut trains, "06:46", 6, true, Vec::new), []);
    }

    #[test]
    fn trains_missing_before_the_window_are_notified_when_it_opens() {
        let mut trains = Trains::default();
        assert_eq!(
            polls(&mut trains, "04:30", 1, false, || vec![j1("onTime")]),
            []
        );
        assert_eq!(polls(&mut trains, "04:32", 10, false, Vec::new), []);
        assert_eq!(
            polls(&mut trains, "05:45", 1, true, Vec::new),
            cancelled(Why::Missing)
        );
    }

    #[test]
    fn a_train_said_cancelled_is_notified_at_once_then_back_if_it_runs() {
        let mut trains = Trains::default();
        assert_eq!(
            polls(&mut trains, "06:00", 1, true, || vec![j1("cancelled")]),
            cancelled(Why::Announced)
        );
        assert_eq!(
            polls(&mut trains, "06:02", 2, true, || vec![j1("cancelled")]),
            []
        );
        // Gone after being said cancelled: nothing more to say.
        assert_eq!(polls(&mut trains, "06:06", 5, true, Vec::new), []);
        assert_eq!(
            polls(&mut trains, "06:16", 1, true, || vec![j1("delayed")]),
            vec![Notification::TrainBack {
                train: train("06:52")
            }]
        );
        assert_eq!(
            polls(&mut trains, "06:18", 2, true, || vec![j1("onTime")]),
            []
        );
    }

    #[test]
    fn a_late_train_is_named_as_last_listed() {
        // Late, then cancelled after its timetable time; back under another mission.
        let mut trains = Trains::default();
        polls(&mut trains, "06:50", 1, true, || {
            vec![visit("j1", "06:52", "07:05", "delayed", 200)]
        });
        let late = Train {
            expected: at("07:05"),
            ..train("06:52")
        };
        assert_eq!(
            polls(&mut trains, "06:58", 1, true, || vec![visit(
                "j1",
                "06:52",
                "07:05",
                "cancelled",
                200
            )]),
            vec![Notification::TrainCancelled {
                train: late.clone(),
                why: Why::Announced
            }]
        );
        let mut changed = visit("j1", "06:52", "07:08", "onTime", 200);
        changed["MonitoredVehicleJourney"]["JourneyNote"] = json!([{ "value": "PILO" }]);
        assert_eq!(
            polls(&mut trains, "07:00", 1, true, || vec![changed.clone()]),
            vec![Notification::TrainBack {
                train: Train {
                    mission: Some("PILO".into()),
                    expected: at("07:08"),
                    ..late
                }
            }]
        );
    }

    #[test]
    fn a_train_said_cancelled_then_lifted_before_the_window_says_nothing() {
        let mut trains = Trains::default();
        polls(&mut trains, "05:00", 1, false, || vec![j1("cancelled")]);
        polls(&mut trains, "05:02", 1, false, || vec![j1("onTime")]);
        assert_eq!(
            polls(&mut trains, "05:45", 1, true, || vec![j1("onTime")]),
            []
        );
    }

    #[test]
    fn only_the_trains_of_the_window_that_take_the_leg_are_followed() {
        let mut trains = Trains::default();
        let board = || {
            vec![
                // Before the window opens, and after it ends.
                visit("j0", "05:40", "05:40", "onTime", 200),
                visit("j9", "07:15", "07:15", "onTime", 200),
                // The other way: it ends elsewhere and is not listed at the alight stop.
                visit("j4", "06:10", "06:10", "onTime", 50),
            ]
        };
        polls(&mut trains, "05:30", 1, true, board);
        assert_eq!(polls(&mut trains, "05:32", 5, true, Vec::new), []);
        assert_eq!(trains.followed, []);
    }

    #[test]
    fn a_train_listed_at_the_alight_stop_later_takes_the_leg() {
        let mut trains = Trains::default();
        let mut seen = seen(vec![visit("j2", "06:20", "06:20", "onTime", 50)]);
        seen[0].arriving = visits(vec![visit("j2", "06:35", "06:35", "onTime", 50)]);
        follow(&mut trains, KEY, window(), at("06:00"), &seen, true);
        assert_eq!(trains.followed.len(), 1);
        // Another line's trains are not this leg's.
        let mut other = Trains::default();
        seen[0].line = "C09999".into();
        follow(&mut other, KEY, window(), at("06:00"), &seen, true);
        assert_eq!(other.followed, []);
    }

    #[test]
    fn a_train_is_forgotten_after_its_time_and_at_a_new_window() {
        let mut trains = Trains::default();
        polls(&mut trains, "06:00", 1, true, || vec![j1("onTime")]);
        polls(&mut trains, "07:21", 1, true, Vec::new);
        assert_eq!(trains.followed.len(), 1);
        polls(&mut trains, "07:22", 1, true, Vec::new);
        assert_eq!(trains.followed, []);

        polls(&mut trains, "06:00", 1, true, || vec![j1("onTime")]);
        let evening = (TUESDAY, Direction::Inbound);
        follow(&mut trains, evening, window(), at("15:00"), &[], true);
        assert_eq!(trains.followed, []);
    }

    /// The train j1 of 6:52 on Tuesday morning, as it ended.
    fn ended(expected: &str, outcome: Outcome, came_back: bool) -> Ended {
        Ended {
            day: TUESDAY,
            direction: Direction::Outbound,
            train: Train {
                expected: at(expected),
                ..train("06:52")
            },
            outcome,
            came_back,
        }
    }

    #[test]
    fn a_train_that_left_ends_as_it_ran_once_forgotten() {
        let mut trains = Trains::default();
        polls(&mut trains, "06:00", 1, true, || {
            vec![visit("j1", "06:52", "06:55", "delayed", 200)]
        });
        polls(&mut trains, "07:24", 1, true, Vec::new);
        assert_eq!(trains.ended, []);
        polls(&mut trains, "07:25", 1, true, Vec::new);
        assert_eq!(trains.ended, [ended("06:55", Outcome::Ran, false)]);
        // Taken by whoever records them: not given twice.
        trains.ended.clear();
        polls(&mut trains, "07:27", 1, true, Vec::new);
        assert_eq!(trains.ended, []);
    }

    #[test]
    fn a_cancelled_train_ends_as_cancelled_and_says_why() {
        let mut missing = Trains::default();
        polls(&mut missing, "05:50", 1, true, || vec![j1("onTime")]);
        polls(&mut missing, "05:52", 4, true, Vec::new);
        polls(&mut missing, "07:22", 1, true, Vec::new);
        assert_eq!(missing.ended, [ended("06:52", Outcome::Missing, false)]);
        // Missing before the window, never notified since: cancelled all the same.
        let mut early = Trains::default();
        polls(&mut early, "04:30", 1, false, || vec![j1("onTime")]);
        polls(&mut early, "04:32", 4, false, Vec::new);
        polls(&mut early, "07:22", 1, false, Vec::new);
        assert_eq!(early.ended, [ended("06:52", Outcome::Missing, false)]);
        let mut announced = Trains::default();
        polls(&mut announced, "06:00", 1, true, || vec![j1("cancelled")]);
        polls(&mut announced, "07:22", 1, true, Vec::new);
        assert_eq!(announced.ended, [ended("06:52", Outcome::Announced, false)]);
    }

    #[test]
    fn a_train_said_cancelled_that_ran_after_all_says_so() {
        let mut trains = Trains::default();
        polls(&mut trains, "06:00", 1, true, || vec![j1("cancelled")]);
        polls(&mut trains, "06:02", 1, true, || vec![j1("onTime")]);
        polls(&mut trains, "07:22", 1, true, Vec::new);
        assert_eq!(trains.ended, [ended("06:52", Outcome::Ran, true)]);
    }

    #[test]
    fn the_trains_of_a_window_end_with_it() {
        let mut trains = Trains::default();
        polls(&mut trains, "06:00", 1, true, || vec![j1("onTime")]);
        let evening = (TUESDAY, Direction::Inbound);
        follow(&mut trains, evening, window(), at("15:00"), &[], true);
        assert_eq!(trains.ended, [ended("06:52", Outcome::Ran, false)]);
        // Remembered with the trains, as they are kept between polls.
        let saved = serde_json::to_string(&trains).unwrap();
        assert!(!saved.contains("ended"), "{saved}");
    }

    #[test]
    fn a_train_without_a_timetable_time_keeps_its_expected_one() {
        let mut trains = Trains::default();
        let mut board = j1("cancelled");
        board["MonitoredVehicleJourney"]["MonitoredCall"]
            .as_object_mut()
            .unwrap()
            .remove("AimedDepartureTime");
        assert_eq!(
            follow(
                &mut trains,
                KEY,
                window(),
                at("06:00"),
                &seen(vec![board]),
                true
            ),
            cancelled(Why::Announced)
        );
        // A call that only arrives here is nothing to board.
        let mut arrival = j1("onTime");
        let call = arrival["MonitoredVehicleJourney"]["MonitoredCall"]
            .as_object_mut()
            .unwrap();
        call.remove("AimedDepartureTime");
        call.remove("ExpectedDepartureTime");
        let mut other = Trains::default();
        follow(
            &mut other,
            KEY,
            window(),
            at("06:00"),
            &seen(vec![arrival]),
            true,
        );
        assert_eq!(other.followed, []);
    }

    #[test]
    fn the_trains_followed_survive_a_restart() {
        let mut trains = Trains::default();
        polls(&mut trains, "06:00", 1, true, || vec![j1("onTime")]);
        let saved = serde_json::to_string(&trains).unwrap();
        assert_eq!(serde_json::from_str::<Trains>(&saved).unwrap(), trains);
    }
}
