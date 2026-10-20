//! When to leave for the train: a reminder a few minutes before.
//!
//! The train is the first one that can be caught when leaving at the usual
//! time of the way, or now if later, up to half an hour after it. The
//! reminder says how it runs, but only what PRIM knows: a train that has not
//! set off yet carries the night's timetable, so it is "still scheduled"
//! rather than "on time". After the reminder, a delay of a few minutes is
//! told once; a train said cancelled gives way to the next one.

use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::alerts::Notification;
use crate::calendar::Direction;
use crate::departures::{CallStatus, Visit, serves};
use crate::trains::{Seen, Train};

/// The reminder comes this long before it is time to leave.
pub const NOTICE: SignedDuration = SignedDuration::from_mins(5);
/// No reminder to leave later than this after the usual time: by then the
/// way was taken, or not at all.
pub const LATEST: SignedDuration = SignedDuration::from_mins(30);
/// A delay this long, said after the reminder, is told.
pub const LATE: SignedDuration = SignedDuration::from_mins(3);
/// A train recorded this recently is followed live.
const LIVE: SignedDuration = SignedDuration::from_mins(30);

/// The reminder of the current window. Kept between polls, and across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leaving {
    window: Option<(Date, Direction)>,
    reminded: Option<Reminded>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Reminded {
    journey: String,
    train: Train,
    told_late: bool,
}

/// One poll's reminders for the leg `seen`, `reach` from leaving to its
/// platform, for someone leaving at `usual`, or now if later.
pub fn remind(
    leaving: &mut Leaving,
    key: (Date, Direction),
    now: Timestamp,
    seen: &Seen,
    reach: SignedDuration,
    usual: Timestamp,
) -> Vec<Notification> {
    if leaving.window != Some(key) {
        leaving.window = Some(key);
        leaving.reminded = None;
    }
    let listed = |journey: &str| {
        seen.board
            .iter()
            .find(|visit| visit.line == seen.line && visit.journey == journey)
    };
    if let Some(reminded) = leaving.reminded.as_mut() {
        match listed(&reminded.journey) {
            // Another train will do: the next one is chosen below.
            Some(visit) if visit.status == CallStatus::Cancelled => {}
            Some(visit) => {
                let expected = departure(visit).unwrap_or(reminded.train.expected);
                let late = expected.duration_since(reminded.train.aimed) >= LATE;
                if late && !reminded.told_late && now < expected {
                    reminded.told_late = true;
                    reminded.train.expected = expected;
                    return vec![Notification::Late {
                        train: reminded.train.clone(),
                    }];
                }
                return vec![];
            }
            None => return vec![],
        }
        leaving.reminded = None;
    }
    // The first train that can be caught, leaving at the usual time or now,
    // but not much later than usual.
    let earliest = usual.max(now);
    let Some((visit, expected)) = seen
        .board
        .iter()
        .filter(|visit| visit.line == seen.line && visit.status != CallStatus::Cancelled)
        .filter_map(|visit| Some((visit, departure(visit)?)))
        .filter(|(visit, expected)| {
            *expected - reach >= earliest
                && *expected - reach <= usual + LATEST
                && serves(visit, *expected, (&seen.alight, &seen.arriving)).is_some()
        })
        .min_by_key(|(_, expected)| *expected)
    else {
        return vec![];
    };
    let at = expected - reach;
    if now < at - NOTICE {
        return vec![];
    }
    let train = Train {
        line: seen.line.clone(),
        mission: visit.mission.clone(),
        destination: visit.destination.clone(),
        from: seen.from.clone(),
        aimed: visit.aimed_departure.unwrap_or(expected),
        expected,
    };
    leaving.reminded = Some(Reminded {
        journey: visit.journey.clone(),
        train: train.clone(),
        told_late: false,
    });
    vec![Notification::Leave {
        train,
        at,
        platform: visit.platform.clone(),
        live: now.duration_since(visit.recorded_at) <= LIVE,
    }]
}

/// A call with no departure ends the journey here: nothing to board.
fn departure(visit: &Visit) -> Option<Timestamp> {
    visit.expected_departure.or(visit.aimed_departure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::departures::parse;
    use jiff::civil::date;
    use serde_json::{Value, json};

    const KEY: (Date, Direction) = (date(2026, 11, 3), Direction::Outbound);
    const REACH: SignedDuration = SignedDuration::from_mins(12);

    /// A time on Tuesday 3 November 2026, in UTC.
    fn at(time: &str) -> Timestamp {
        format!("2026-11-03T{time}:00Z").parse().unwrap()
    }

    /// Leaving at 7:20 UTC or later, the 7:32 train is the first one to catch.
    fn usual() -> Timestamp {
        at("07:20")
    }

    /// A train of line L to the alight stop (SP:200), recorded at `recorded`.
    fn visit(journey: &str, aimed: &str, expected: &str, status: &str, recorded: &str) -> Value {
        json!({
            "RecordedAtTime": at(recorded).to_string(),
            "MonitoredVehicleJourney": {
                "LineRef": { "value": "STIF:Line::C01740:" },
                "FramedVehicleJourneyRef": { "DatedVehicleJourneyRef": journey },
                "DestinationRef": { "value": "STIF:StopArea:SP:200:" },
                "DestinationName": [{ "value": "Gare Centrale" }],
                "JourneyNote": [{ "value": "PALO" }],
                "MonitoredCall": {
                    "AimedDepartureTime": at(aimed).to_string(),
                    "ExpectedDepartureTime": at(expected).to_string(),
                    "DepartureStatus": status,
                    "DeparturePlatformName": { "value": "2" }
                }
            }
        })
    }

    fn seen(board: Vec<Value>) -> Seen {
        let answer = json!({ "Siri": { "ServiceDelivery": { "StopMonitoringDelivery": [{ "MonitoredStopVisit": board }] } } });
        Seen {
            line: "C01740".into(),
            from: "Les Aubépines".into(),
            alight: "STIF:StopArea:SP:200:".into(),
            board: parse(&serde_json::to_vec(&answer).unwrap()).unwrap(),
            arriving: vec![],
        }
    }

    /// The trains of 7:17, 7:32 and 7:47, live since 7:00 unless said otherwise.
    fn three(second: Value) -> Vec<Value> {
        vec![
            visit("j1", "07:17", "07:17", "onTime", "07:00"),
            second,
            visit("j3", "07:47", "07:47", "onTime", "07:00"),
        ]
    }

    fn on_time() -> Value {
        visit("j2", "07:32", "07:32", "onTime", "07:00")
    }

    fn train(aimed: &str, expected: &str) -> Train {
        Train {
            line: "C01740".into(),
            mission: Some("PALO".into()),
            destination: "Gare Centrale".into(),
            from: "Les Aubépines".into(),
            aimed: at(aimed),
            expected: at(expected),
        }
    }

    fn leave(train: Train, at_: &str, live: bool) -> Vec<Notification> {
        vec![Notification::Leave {
            train,
            at: at(at_),
            platform: Some("2".into()),
            live,
        }]
    }

    fn poll(leaving: &mut Leaving, now: &str, board: Vec<Value>) -> Vec<Notification> {
        remind(leaving, KEY, at(now), &seen(board), REACH, usual())
    }

    #[test]
    fn the_reminder_comes_five_minutes_before_leaving_for_the_first_train_to_catch() {
        let mut leaving = Leaving::default();
        // Leave at 7:20 for the 7:32: reminded from 7:15.
        assert_eq!(poll(&mut leaving, "07:14", three(on_time())), []);
        assert_eq!(
            poll(&mut leaving, "07:15", three(on_time())),
            leave(train("07:32", "07:32"), "07:20", true)
        );
        // Once.
        assert_eq!(poll(&mut leaving, "07:17", three(on_time())), []);
    }

    #[test]
    fn a_train_not_followed_live_yet_is_only_still_scheduled() {
        let mut leaving = Leaving::default();
        let night = visit("j2", "07:32", "07:32", "onTime", "00:10");
        assert_eq!(
            poll(&mut leaving, "07:16", three(night)),
            leave(train("07:32", "07:32"), "07:20", false)
        );
    }

    #[test]
    fn a_late_train_moves_the_time_to_leave() {
        // The 7:17 now leaves at 7:21: still too soon, with the platform 12 minutes away.
        // The 7:32, 2 minutes late, is the one: leave at 7:22.
        let mut leaving = Leaving::default();
        let board = vec![
            visit("j1", "07:17", "07:21", "delayed", "07:10"),
            visit("j2", "07:32", "07:34", "delayed", "07:10"),
        ];
        assert_eq!(poll(&mut leaving, "07:16", board.clone()), []);
        assert_eq!(
            poll(&mut leaving, "07:17", board),
            leave(train("07:32", "07:34"), "07:22", true)
        );
    }

    #[test]
    fn a_cancelled_train_gives_way_to_the_next_even_after_the_reminder() {
        let mut leaving = Leaving::default();
        let cancelled = visit("j2", "07:32", "07:32", "cancelled", "07:05");
        assert_eq!(
            poll(&mut leaving, "07:30", three(cancelled.clone())),
            leave(train("07:47", "07:47"), "07:35", true)
        );

        let mut leaving = Leaving::default();
        poll(&mut leaving, "07:15", three(on_time()));
        assert_eq!(poll(&mut leaving, "07:17", three(cancelled.clone())), []);
        assert_eq!(
            poll(&mut leaving, "07:30", three(cancelled)),
            leave(train("07:47", "07:47"), "07:35", true)
        );
    }

    #[test]
    fn a_delay_after_the_reminder_is_told_once() {
        let mut leaving = Leaving::default();
        poll(&mut leaving, "07:15", three(on_time()));
        let late = |minutes: &str| visit("j2", "07:32", minutes, "delayed", "07:16");
        assert_eq!(poll(&mut leaving, "07:16", three(late("07:34"))), []);
        assert_eq!(
            poll(&mut leaving, "07:18", three(late("07:35"))),
            vec![Notification::Late {
                train: train("07:32", "07:35")
            }]
        );
        assert_eq!(poll(&mut leaving, "07:20", three(late("07:40"))), []);
        // Gone from the list, or past: nothing more.
        assert_eq!(poll(&mut leaving, "07:22", vec![]), []);
    }

    #[test]
    fn a_train_already_gone_or_not_for_this_leg_is_not_a_reminder() {
        let mut leaving = Leaving::default();
        // Past its time, or not leaving from here, or the other way.
        let board = vec![
            visit("j1", "07:17", "07:17", "onTime", "07:00"),
            json!({
                "RecordedAtTime": at("07:00").to_string(),
                "MonitoredVehicleJourney": {
                    "LineRef": { "value": "STIF:Line::C01740:" },
                    "FramedVehicleJourneyRef": { "DatedVehicleJourneyRef": "j5" },
                    "DestinationRef": { "value": "STIF:StopArea:SP:50:" },
                    "MonitoredCall": { "AimedDepartureTime": at("07:40").to_string() }
                }
            }),
            // Ends here: nothing to board.
            json!({
                "RecordedAtTime": at("07:00").to_string(),
                "MonitoredVehicleJourney": {
                    "LineRef": { "value": "STIF:Line::C01740:" },
                    "FramedVehicleJourneyRef": { "DatedVehicleJourneyRef": "j6" },
                    "DestinationRef": { "value": "STIF:StopArea:SP:100:" },
                    "MonitoredCall": { "AimedArrivalTime": at("07:45").to_string() }
                }
            }),
        ];
        assert_eq!(poll(&mut leaving, "07:30", board), []);
        // Polls resuming at 7:25, too late for the 7:32: the 7:47 it is.
        let mut leaving = Leaving::default();
        assert_eq!(poll(&mut leaving, "07:25", three(on_time())), []);
        assert_eq!(
            poll(&mut leaving, "07:30", three(on_time())),
            leave(train("07:47", "07:47"), "07:35", true)
        );
        // After it has left, nothing.
        let mut leaving = Leaving::default();
        assert_eq!(poll(&mut leaving, "07:50", three(on_time())), []);
    }

    #[test]
    fn no_reminder_to_leave_more_than_half_an_hour_after_the_usual_time() {
        let board = || {
            vec![
                visit("j3", "07:47", "07:47", "onTime", "07:30"),
                visit("j4", "08:02", "08:02", "onTime", "07:30"),
                visit("j5", "08:17", "08:17", "onTime", "07:30"),
            ]
        };
        // Polls resuming late: leaving at 7:50 is still the way.
        let mut leaving = Leaving::default();
        assert_eq!(
            poll(&mut leaving, "07:45", board()),
            leave(train("08:02", "08:02"), "07:50", true)
        );
        // Leaving at 8:05 no more: by then the way was taken, or not at all.
        let mut leaving = Leaving::default();
        assert_eq!(poll(&mut leaving, "08:00", board()), []);
    }

    #[test]
    fn a_new_window_starts_afresh_and_survives_a_restart() {
        let mut leaving = Leaving::default();
        poll(&mut leaving, "07:15", three(on_time()));
        let saved = serde_json::to_string(&leaving).unwrap();
        assert_eq!(serde_json::from_str::<Leaving>(&saved).unwrap(), leaving);
        let evening = (KEY.0, Direction::Inbound);
        let reminded = remind(
            &mut leaving,
            evening,
            at("07:15"),
            &seen(three(on_time())),
            REACH,
            usual(),
        );
        assert_eq!(reminded, leave(train("07:32", "07:32"), "07:20", true));
    }
}
