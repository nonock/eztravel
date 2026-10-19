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
    /// The operator says so.
    Announced,
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

/// The trains followed in the current window. Kept between polls, and across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trains {
    window: Option<(Date, Direction)>,
    followed: Vec<Followed>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Followed {
    journey: String,
    train: Train,
    /// Polls in a row without it, while it could not have left.
    missing: u32,
    said_cancelled: bool,
    notified: bool,
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
    if trains.window != Some(key) {
        trains.window = Some(key);
        trains.followed.clear();
    }
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
            if let Some(followed) = trains
                .followed
                .iter_mut()
                .find(|followed| followed.journey == visit.journey)
            {
                // A train being changed may come back under another mission.
                followed.train.mission = visit.mission.clone();
                followed.train.destination = visit.destination.clone();
                followed.train.expected = expected;
                followed.missing = 0;
                followed.said_cancelled = said_cancelled;
                if followed.notified && !said_cancelled {
                    followed.notified = false;
                    notifications.push(Notification::TrainBack {
                        train: followed.train.clone(),
                    });
                }
                continue;
            }
            let aimed = visit.aimed_departure.unwrap_or(expected);
            let in_window = window.0 <= aimed && aimed < window.1;
            if !in_window || serves(visit, expected, (&leg.alight, &leg.arriving)).is_none() {
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
            });
        }
    }
    for followed in &mut trains.followed {
        if !listed.contains(&followed.journey.as_str())
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
            Why::Missing
        } else {
            continue;
        };
        followed.notified = true;
        notifications.push(Notification::TrainCancelled {
            train: followed.train.clone(),
            why,
        });
    }
    trains
        .followed
        .retain(|followed| now.duration_since(followed.train.expected) < FORGET_AFTER);
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
