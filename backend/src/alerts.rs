//! When to notify, poll after poll.
//!
//! During a watch window, each line of the commute is either fine or
//! disrupted at some severity. A notification goes out when that changes:
//! a disruption starts (or is already there when the window opens), its
//! severity changes, or it ends. Disruption records come and go under new
//! ids as the operator edits them, so the line's state is what is tracked,
//! not individual records.
//!
//! Planned disruptions (works, adapted timetables over several days) would
//! keep a line "disrupted" all day: they are announced once a day instead.
//! The evening before an office day, what will disrupt the next day's windows
//! is announced too, each record once.

use jiff::civil::{Date, DateTime};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::calendar::{Direction, WatchWindows};
use crate::disruptions::{Cause, Disruption, Severity};
use crate::relevance::{Bearing, Leg, Route, Scope, affects, bearing, scope};
use crate::trains::{Train, Why};

/// One ride of the commute, with the routes of its line.
pub struct WatchedLeg {
    pub leg: Leg,
    pub routes: Vec<Route>,
}

pub struct Commute {
    pub outbound: Vec<WatchedLeg>,
    pub inbound: Vec<WatchedLeg>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notification {
    /// `scope` says where the record shown is, on the leg: see [`shown`].
    Started {
        line: String,
        severity: Severity,
        disruptions: Vec<Disruption>,
        scope: Scope,
    },
    Changed {
        line: String,
        from: Severity,
        to: Severity,
        disruptions: Vec<Disruption>,
        scope: Scope,
    },
    Ended {
        line: String,
    },
    Planned {
        disruptions: Vec<Disruption>,
    },
    /// The evening before an office day, `date`: what will disrupt its windows.
    Tomorrow {
        date: Date,
        disruptions: Vec<Disruption>,
    },
    /// No complete check for a while: no news no longer means good news.
    Outage,
    /// Checks work again, after an outage was notified.
    Recovered,
    /// A train of the window will not run.
    TrainCancelled {
        train: Train,
        why: Why,
    },
    /// A train notified as cancelled runs after all.
    TrainBack {
        train: Train,
    },
    /// Time to leave for `train` `at` the given time. `live` when PRIM follows it live.
    Leave {
        train: Train,
        at: Timestamp,
        platform: Option<String>,
        live: bool,
    },
    /// The train of the reminder now runs late.
    Late {
        train: Train,
    },
}

/// What has been notified so far. Kept between polls, and across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    window: Option<(Date, Direction)>,
    lines: Vec<(String, Severity)>,
    planned_day: Option<Date>,
    planned_titles: Vec<String>,
    /// The office day announced the evening before, last.
    #[serde(default)]
    eve_day: Option<Date>,
    /// The records announced the evening before, by title, while still in the feed.
    #[serde(default)]
    eve_titles: Vec<String>,
}

impl State {
    /// How disrupted `line` is in the current window, as last notified; `None` when fine.
    pub fn severity(&self, line: &str) -> Option<Severity> {
        self.lines
            .iter()
            .find(|(known, _)| known == line)
            .map(|(_, severity)| *severity)
    }

    /// The lines disrupted in the current window, as last notified.
    pub fn lines(&self) -> &[(String, Severity)] {
        &self.lines
    }

    /// The office day whose trouble was announced the evening before, last.
    pub fn eve(&self) -> Option<Date> {
        self.eve_day
    }

    /// The planned disruptions announced on `today`.
    pub fn planned(&self, today: Date) -> &[String] {
        if self.planned_day == Some(today) {
            &self.planned_titles
        } else {
            &[]
        }
    }
}

/// `now` is in network local time; `direction` is the one being watched, if any.
pub fn decide(
    state: &mut State,
    now: DateTime,
    direction: Option<Direction>,
    commute: &Commute,
    disruptions: &[Disruption],
) -> Vec<Notification> {
    let Some(direction) = direction else {
        state.window = None;
        state.lines.clear();
        return vec![];
    };
    let today = now.date();
    if state.window != Some((today, direction)) {
        // A new window: whatever is under way is news again.
        state.window = Some((today, direction));
        state.lines.clear();
    }
    if state.planned_day != Some(today) {
        state.planned_day = Some(today);
        state.planned_titles.clear();
    }
    let legs = match direction {
        Direction::Outbound => &commute.outbound,
        Direction::Inbound => &commute.inbound,
    };
    let current: Vec<&Disruption> = disruptions
        .iter()
        .filter(|disruption| {
            disruption.is_active(now) && disruption.severity != Severity::Information
        })
        .collect();

    let mut notifications = vec![];
    let mut announced = vec![];
    for disruption in current
        .iter()
        .filter(|disruption| is_planned(disruption, now))
    {
        // Records of a planned disruption get new ids as they are edited: the title is what stays.
        if !state.planned_titles.contains(&disruption.title)
            && legs
                .iter()
                .any(|watched| affects(disruption, &watched.leg, &watched.routes))
        {
            state.planned_titles.push(disruption.title.clone());
            announced.push((*disruption).clone());
        }
    }
    if !announced.is_empty() {
        notifications.push(Notification::Planned {
            disruptions: announced,
        });
    }

    for watched in legs {
        let line = watched.leg.line.clone();
        let incidents: Vec<Disruption> = current
            .iter()
            .filter(|disruption| {
                !is_planned(disruption, now) && affects(disruption, &watched.leg, &watched.routes)
            })
            .map(|disruption| (*disruption).clone())
            .collect();
        let severity = incidents
            .iter()
            .map(|disruption| level(disruption.severity))
            .max_by_key(|severity| *severity == Severity::Blocking);
        let previous = state.severity(&line);
        state.lines.retain(|(known, _)| *known != line);
        if let Some(severity) = severity {
            state.lines.push((line.clone(), severity));
        }
        let scope = |severity| {
            shown(severity, &incidents).map_or(Scope::Line, |record| {
                scope(record, &watched.leg, &watched.routes)
            })
        };
        match (previous, severity) {
            (None, Some(severity)) => {
                notifications.push(Notification::Started {
                    line,
                    severity,
                    scope: scope(severity),
                    disruptions: incidents,
                });
            }
            (Some(from), Some(to)) if from != to => {
                notifications.push(Notification::Changed {
                    line,
                    from,
                    to,
                    scope: scope(to),
                    disruptions: incidents,
                });
            }
            (Some(_), None) => notifications.push(Notification::Ended { line }),
            _ => {}
        }
    }
    notifications
}

/// The evening before `tomorrow`, an office day: the records more than
/// information that touch a leg during one of its windows, each once.
pub fn eve(
    state: &mut State,
    tomorrow: Date,
    windows: &WatchWindows,
    commute: &Commute,
    disruptions: &[Disruption],
) -> Vec<Notification> {
    state.eve_day = Some(tomorrow);
    state.eve_titles.retain(|title| {
        disruptions
            .iter()
            .any(|disruption| disruption.title == *title)
    });
    let mut announced: Vec<Disruption> = vec![];
    for (window, legs) in [
        (windows.outbound, &commute.outbound),
        (windows.inbound, &commute.inbound),
    ] {
        let (start, end) = (
            tomorrow.to_datetime(window.start),
            tomorrow.to_datetime(window.end),
        );
        for disruption in disruptions {
            let during = disruption
                .periods
                .iter()
                .any(|period| period.begin < end && start < period.end);
            if during
                && disruption.severity != Severity::Information
                && !state.eve_titles.contains(&disruption.title)
                && legs
                    .iter()
                    .any(|watched| affects(disruption, &watched.leg, &watched.routes))
            {
                state.eve_titles.push(disruption.title.clone());
                announced.push(disruption.clone());
            }
        }
    }
    if announced.is_empty() {
        return vec![];
    }
    vec![Notification::Tomorrow {
        date: tomorrow,
        disruptions: announced,
    }]
}

/// The record a line's notification shows, of those behind its `severity`:
/// the first as bad, or else the first.
pub fn shown(severity: Severity, disruptions: &[Disruption]) -> Option<&Disruption> {
    disruptions
        .iter()
        .find(|disruption| disruption.severity == severity)
        .or(disruptions.first())
}

/// Works, or anything whose current period lasts a day or more.
fn is_planned(disruption: &Disruption, now: DateTime) -> bool {
    disruption.cause == Cause::Works
        || disruption.periods.iter().any(|period| {
            period.begin <= now
                && now < period.end
                && period.end.duration_since(period.begin) >= SignedDuration::from_hours(24)
        })
}

/// How badly `leg` is disrupted at `now`, in network local time, for its
/// backups: by an incident under way, as the line's state in a window, or by
/// planned works that cut it, on the leg itself for sure. None when it runs,
/// or is only slowed by works.
pub fn severity_on(
    disruptions: &[Disruption],
    now: DateTime,
    leg: &Leg,
    routes: &[Route],
) -> Option<Severity> {
    disruptions
        .iter()
        .filter(|disruption| {
            disruption.is_active(now) && disruption.severity != Severity::Information
        })
        .filter(|disruption| {
            !is_planned(disruption, now) || disruption.severity == Severity::Blocking
        })
        // A detour is not taken on a guess, nor for trains beyond the leg.
        .filter(|disruption| bearing(disruption, leg, routes) == Bearing::On)
        .map(|disruption| level(disruption.severity))
        .max_by_key(|severity| *severity == Severity::Blocking)
}

/// Severity on a two-step scale: a record of unknown severity counts as disrupted.
fn level(severity: Severity) -> Severity {
    match severity {
        Severity::Blocking => Severity::Blocking,
        _ => Severity::Disrupted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::Window;
    use crate::disruptions::{Period, Place, Section};
    use crate::relevance::RouteStop;
    use jiff::civil::{date, time};

    const TRAIN: &str = "C01740";
    const METRO: &str = "C02000";
    const OTHER: &str = "C09999";

    fn leg(line: &str) -> WatchedLeg {
        // Without routes, every disruption of the line counts (see `relevance`).
        WatchedLeg {
            leg: Leg {
                line: line.into(),
                board: "sa:A".into(),
                alight: "sa:B".into(),
                reach: None,
            },
            routes: vec![],
        }
    }

    fn commute() -> Commute {
        Commute {
            outbound: vec![leg(TRAIN), leg(METRO)],
            inbound: vec![leg(METRO), leg(TRAIN)],
        }
    }

    fn at(day: i8, hour: i8, minute: i8) -> DateTime {
        date(2026, 11, day).at(hour, minute, 0, 0)
    }

    fn disruption(
        id: &str,
        line: &str,
        severity: Severity,
        from: DateTime,
        to: DateTime,
    ) -> Disruption {
        Disruption {
            id: id.into(),
            lines: vec![line.into()],
            stops: vec![],
            periods: vec![Period {
                begin: from,
                end: to,
            }],
            updated: from,
            cause: Cause::Incident,
            severity,
            title: format!("Incident {id}"),
            short_message: Some("Trafic perturbé".into()),
            message: format!("<p>Texte {id}</p>"),
            sections: Vec::<Section>::new(),
        }
    }

    /// An incident on the train line from 7:00 to 10:00 on Tuesday 3 November.
    fn incident(id: &str, severity: Severity) -> Disruption {
        disruption(id, TRAIN, severity, at(3, 7, 0), at(3, 10, 0))
    }

    fn planned(id: &str, cause: Cause, from: DateTime, to: DateTime) -> Disruption {
        Disruption {
            cause,
            title: format!("Prévu {id}"),
            ..disruption(id, TRAIN, Severity::Disrupted, from, to)
        }
    }

    fn poll(
        state: &mut State,
        now: DateTime,
        direction: Option<Direction>,
        disruptions: &[Disruption],
    ) -> Vec<Notification> {
        decide(state, now, direction, &commute(), disruptions)
    }

    const MORNING: Option<Direction> = Some(Direction::Outbound);
    const EVENING: Option<Direction> = Some(Direction::Inbound);

    fn started(line: &str, severity: Severity, disruptions: &[Disruption]) -> Notification {
        Notification::Started {
            line: line.into(),
            severity,
            disruptions: disruptions.to_vec(),
            // Records without section nor stop.
            scope: Scope::Line,
        }
    }

    #[test]
    fn an_incident_under_way_when_the_window_opens_is_notified_at_once() {
        let mut state = State::default();
        let current = [incident("a", Severity::Disrupted)];
        assert_eq!(
            poll(&mut state, at(3, 7, 30), MORNING, &current),
            [started(TRAIN, Severity::Disrupted, &current)]
        );
    }

    #[test]
    fn nothing_new_nothing_sent() {
        let mut state = State::default();
        let current = [incident("a", Severity::Disrupted)];
        poll(&mut state, at(3, 7, 30), MORNING, &current);
        assert_eq!(poll(&mut state, at(3, 7, 32), MORNING, &current), []);
        assert_eq!(
            poll(&mut state, at(3, 7, 34), MORNING, &[]),
            [Notification::Ended { line: TRAIN.into() }]
        );
        assert_eq!(poll(&mut state, at(3, 7, 36), MORNING, &[]), []);
    }

    #[test]
    fn a_new_record_with_the_same_severity_is_not_news() {
        // The operator edits the text: the record comes back under a new id.
        let mut state = State::default();
        poll(
            &mut state,
            at(3, 7, 30),
            MORNING,
            &[incident("a", Severity::Disrupted)],
        );
        assert_eq!(
            poll(
                &mut state,
                at(3, 7, 32),
                MORNING,
                &[incident("b", Severity::Disrupted)]
            ),
            []
        );
    }

    #[test]
    fn a_change_of_severity_is_notified_both_ways() {
        let mut state = State::default();
        poll(
            &mut state,
            at(3, 7, 30),
            MORNING,
            &[incident("a", Severity::Disrupted)],
        );
        let worse = [incident("b", Severity::Blocking)];
        assert_eq!(
            poll(&mut state, at(3, 7, 32), MORNING, &worse),
            [Notification::Changed {
                line: TRAIN.into(),
                from: Severity::Disrupted,
                to: Severity::Blocking,
                disruptions: worse.to_vec(),
                scope: Scope::Line,
            }]
        );
        let better = [incident("c", Severity::Disrupted)];
        assert_eq!(
            poll(&mut state, at(3, 7, 34), MORNING, &better),
            [Notification::Changed {
                line: TRAIN.into(),
                from: Severity::Blocking,
                to: Severity::Disrupted,
                disruptions: better.to_vec(),
                scope: Scope::Line,
            }]
        );
    }

    #[test]
    fn the_worst_record_sets_the_severity_and_all_are_sent() {
        let mut state = State::default();
        let current = [
            incident("a", Severity::Disrupted),
            incident("b", Severity::Blocking),
        ];
        assert_eq!(
            poll(&mut state, at(3, 7, 30), MORNING, &current),
            [started(TRAIN, Severity::Blocking, &current)]
        );
    }

    #[test]
    fn an_unknown_severity_counts_as_disrupted_and_information_as_nothing() {
        let mut state = State::default();
        let unknown = [incident("a", Severity::Unknown)];
        assert_eq!(
            poll(&mut state, at(3, 7, 30), MORNING, &unknown),
            [started(TRAIN, Severity::Disrupted, &unknown)]
        );
        let mut state = State::default();
        assert_eq!(
            poll(
                &mut state,
                at(3, 7, 30),
                MORNING,
                &[incident("b", Severity::Information)]
            ),
            []
        );
    }

    #[test]
    fn ended_records_still_listed_are_ignored() {
        // The feed keeps a record listed after its period: here it ended at 7:00.
        let mut state = State::default();
        let ended = disruption("a", TRAIN, Severity::Blocking, at(3, 6, 0), at(3, 7, 0));
        assert_eq!(poll(&mut state, at(3, 7, 30), MORNING, &[ended]), []);
    }

    #[test]
    fn other_lines_are_ignored() {
        let mut state = State::default();
        let elsewhere = disruption("a", OTHER, Severity::Blocking, at(3, 7, 0), at(3, 10, 0));
        assert_eq!(poll(&mut state, at(3, 7, 30), MORNING, &[elsewhere]), []);
    }

    #[test]
    fn each_line_of_the_commute_has_its_own_state() {
        let mut state = State::default();
        let train = incident("a", Severity::Disrupted);
        let metro = disruption("m", METRO, Severity::Blocking, at(3, 7, 0), at(3, 10, 0));
        assert_eq!(
            poll(
                &mut state,
                at(3, 7, 30),
                MORNING,
                &[train.clone(), metro.clone()]
            ),
            [
                started(TRAIN, Severity::Disrupted, std::slice::from_ref(&train)),
                started(METRO, Severity::Blocking, &[metro])
            ]
        );
        // On the way back, legs come in the other order.
        let evening = disruption("m2", METRO, Severity::Disrupted, at(3, 17, 0), at(3, 20, 0));
        let train_evening =
            disruption("t2", TRAIN, Severity::Disrupted, at(3, 17, 0), at(3, 20, 0));
        assert_eq!(
            poll(
                &mut state,
                at(3, 17, 30),
                EVENING,
                &[train_evening.clone(), evening.clone()]
            ),
            [
                started(METRO, Severity::Disrupted, &[evening]),
                started(TRAIN, Severity::Disrupted, &[train_evening])
            ]
        );
    }

    #[test]
    fn nothing_outside_the_windows_and_no_end_sent_when_a_window_closes() {
        let mut state = State::default();
        let current = [incident("a", Severity::Disrupted)];
        poll(&mut state, at(3, 7, 30), MORNING, &current);
        assert_eq!(poll(&mut state, at(3, 9, 30), None, &current), []);
        assert_eq!(poll(&mut state, at(3, 9, 32), None, &[]), []);
    }

    #[test]
    fn each_window_starts_afresh() {
        let mut state = State::default();
        let all_day = [disruption(
            "a",
            TRAIN,
            Severity::Disrupted,
            at(3, 6, 0),
            at(3, 23, 0),
        )];
        poll(&mut state, at(3, 7, 30), MORNING, &all_day);
        poll(&mut state, at(3, 12, 0), None, &all_day);
        assert_eq!(
            poll(&mut state, at(3, 17, 30), EVENING, &all_day),
            [started(TRAIN, Severity::Disrupted, &all_day)]
        );
        // The next morning too, even without a poll in between.
        let next_day = [disruption(
            "b",
            TRAIN,
            Severity::Disrupted,
            at(4, 6, 0),
            at(4, 23, 0),
        )];
        assert_eq!(
            poll(&mut state, at(4, 7, 30), MORNING, &next_day),
            [started(TRAIN, Severity::Disrupted, &next_day)]
        );
    }

    #[test]
    fn planned_works_are_announced_once_a_day() {
        let mut state = State::default();
        let works = [planned("w", Cause::Works, at(3, 5, 0), at(3, 12, 0))];
        assert_eq!(
            poll(&mut state, at(3, 7, 30), MORNING, &works),
            [Notification::Planned {
                disruptions: works.to_vec()
            }]
        );
        assert_eq!(poll(&mut state, at(3, 7, 32), MORNING, &works), []);
        let all_day = [planned("w", Cause::Works, at(3, 5, 0), at(3, 23, 0))];
        assert_eq!(poll(&mut state, at(3, 17, 30), EVENING, &all_day), []);
        let tomorrow = [planned("w", Cause::Works, at(4, 5, 0), at(4, 23, 0))];
        assert_eq!(
            poll(&mut state, at(4, 7, 30), MORNING, &tomorrow),
            [Notification::Planned {
                disruptions: tomorrow.to_vec()
            }]
        );
    }

    #[test]
    fn a_disruption_over_a_day_long_is_planned_whatever_its_cause() {
        // Seen on line J: an adapted timetable over a week, filed as an incident.
        let mut state = State::default();
        let week = [planned("o", Cause::Incident, at(2, 3, 0), at(9, 3, 0))];
        assert_eq!(
            poll(&mut state, at(3, 7, 30), MORNING, &week),
            [Notification::Planned {
                disruptions: week.to_vec()
            }]
        );
        // It does not hide an incident starting on the same line.
        let both = [week[0].clone(), incident("a", Severity::Disrupted)];
        assert_eq!(
            poll(&mut state, at(3, 7, 32), MORNING, &both),
            [started(TRAIN, Severity::Disrupted, &both[1..])]
        );
    }

    #[test]
    fn a_planned_disruption_announced_later_in_the_day_is_sent_on_its_own() {
        let mut state = State::default();
        let first = planned("w", Cause::Works, at(3, 5, 0), at(3, 23, 0));
        poll(
            &mut state,
            at(3, 7, 30),
            MORNING,
            std::slice::from_ref(&first),
        );
        let second = planned("x", Cause::Works, at(3, 16, 0), at(3, 23, 0));
        assert_eq!(
            poll(&mut state, at(3, 17, 30), EVENING, &[first, second.clone()]),
            [Notification::Planned {
                disruptions: vec![second]
            }]
        );
    }

    #[test]
    fn the_state_survives_a_restart() {
        let mut state = State::default();
        let current = [
            incident("a", Severity::Blocking),
            planned("w", Cause::Works, at(3, 5, 0), at(3, 12, 0)),
        ];
        poll(&mut state, at(3, 7, 30), MORNING, &current);
        let saved = serde_json::to_string(&state).unwrap();
        let mut restored: State = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored, state);
        // Nothing is sent twice because the service restarted.
        assert_eq!(poll(&mut restored, at(3, 7, 32), MORNING, &current), []);
    }

    fn windows() -> WatchWindows {
        let window = |start, end| Window {
            start,
            end,
            trains_from: None,
        };
        WatchWindows {
            outbound: window(time(6, 45, 0, 0), time(8, 15, 0, 0)),
            inbound: window(time(16, 0, 0, 0), time(17, 30, 0, 0)),
            eve: Some(time(20, 0, 0, 0)),
            late: vec![],
        }
    }

    fn eve(state: &mut State, tomorrow: i8, disruptions: &[Disruption]) -> Vec<Notification> {
        super::eve(
            state,
            date(2026, 11, tomorrow),
            &windows(),
            &commute(),
            disruptions,
        )
    }

    #[test]
    fn the_evening_before_what_touches_the_next_days_windows_is_announced() {
        let mut state = State::default();
        let works = planned("w", Cause::Works, at(3, 5, 0), at(3, 12, 0));
        let strike = Disruption {
            title: "Grève".into(),
            ..disruption("s", METRO, Severity::Blocking, at(3, 0, 0), at(4, 0, 0))
        };
        // At night, between the windows, today, on another line, or information only: not.
        let night = planned("n", Cause::Works, at(3, 22, 0), at(4, 5, 0));
        let midday = planned("m", Cause::Works, at(3, 9, 0), at(3, 15, 0));
        let today = planned("t", Cause::Works, at(2, 6, 0), at(2, 18, 0));
        let elsewhere = Disruption {
            lines: vec![OTHER.into()],
            ..planned("o", Cause::Works, at(3, 5, 0), at(3, 23, 0))
        };
        let notice = Disruption {
            severity: Severity::Information,
            ..planned("i", Cause::Works, at(3, 5, 0), at(3, 23, 0))
        };
        // The way home only: still the day's trouble.
        let evening = planned("e", Cause::Works, at(3, 17, 0), at(3, 19, 0));
        let feed = [
            works.clone(),
            night,
            strike.clone(),
            midday,
            today,
            elsewhere,
            notice,
            evening.clone(),
        ];
        assert_eq!(
            eve(&mut state, 3, &feed),
            [Notification::Tomorrow {
                date: date(2026, 11, 3),
                disruptions: vec![works, strike, evening],
            }]
        );
        assert_eq!(state.eve(), Some(date(2026, 11, 3)));
    }

    #[test]
    fn each_record_is_announced_the_evening_before_once() {
        let mut state = State::default();
        let week = planned("w", Cause::Works, at(3, 5, 0), at(7, 5, 0));
        assert_eq!(eve(&mut state, 3, std::slice::from_ref(&week)).len(), 1);
        // The next evening, only what is new.
        let strike = Disruption {
            title: "Grève".into(),
            ..disruption("s", METRO, Severity::Blocking, at(4, 0, 0), at(5, 0, 0))
        };
        assert_eq!(
            eve(&mut state, 4, &[week.clone(), strike.clone()]),
            [Notification::Tomorrow {
                date: date(2026, 11, 4),
                disruptions: vec![strike],
            }]
        );
        // A record gone from the feed is forgotten: back later, it is news again.
        assert_eq!(eve(&mut state, 5, &[]), []);
        assert_eq!(state.eve(), Some(date(2026, 11, 5)));
        assert_eq!(eve(&mut state, 6, std::slice::from_ref(&week)).len(), 1);
    }

    #[test]
    fn a_state_saved_by_the_previous_version_is_read() {
        // The service on fly.io keeps its state across deploys: the shape must not change.
        let saved = r#"{"window":["2026-11-03","Outbound"],"lines":[["C01740","BLOQUANTE"]],"planned_day":"2026-11-03","planned_titles":["Prévu w"]}"#;
        let state: State = serde_json::from_str(saved).unwrap();
        assert_eq!(state.severity(TRAIN), Some(Severity::Blocking));
        assert_eq!(state.planned(date(2026, 11, 3)), ["Prévu w"]);
        assert_eq!(state.eve(), None);
    }

    #[test]
    fn the_state_tells_each_line_and_what_is_planned_today() {
        let mut state = State::default();
        let current = [
            incident("a", Severity::Disrupted),
            planned("w", Cause::Works, at(3, 5, 0), at(3, 12, 0)),
        ];
        poll(&mut state, at(3, 7, 30), MORNING, &current);
        assert_eq!(state.severity(TRAIN), Some(Severity::Disrupted));
        assert_eq!(state.severity(METRO), None);
        assert_eq!(state.planned(date(2026, 11, 3)), ["Prévu w"]);
        // Announced yesterday is not planned today.
        assert_eq!(state.planned(date(2026, 11, 4)), Vec::<String>::new());
    }

    #[test]
    fn planned_information_only_is_not_announced() {
        let mut state = State::default();
        let notice = Disruption {
            severity: Severity::Information,
            ..planned("i", Cause::Works, at(3, 5, 0), at(3, 23, 0))
        };
        assert_eq!(poll(&mut state, at(3, 7, 30), MORNING, &[notice]), []);
    }

    #[test]
    fn a_leg_is_disrupted_by_incidents_and_by_planned_works_that_cut_it() {
        let train = leg(TRAIN);
        let on_train =
            |disruptions: &[Disruption]| severity_on(disruptions, at(3, 7, 30), &train.leg, &[]);
        assert_eq!(on_train(&[]), None);
        // Incidents under way, as in a window: the worst counts.
        assert_eq!(
            on_train(&[incident("a", Severity::Disrupted)]),
            Some(Severity::Disrupted)
        );
        assert_eq!(
            on_train(&[
                incident("a", Severity::Unknown),
                incident("b", Severity::Blocking)
            ]),
            Some(Severity::Blocking)
        );
        assert_eq!(
            on_train(&[incident("a", Severity::Unknown)]),
            Some(Severity::Disrupted)
        );
        // Information, over, or on another line: nothing.
        assert_eq!(on_train(&[incident("a", Severity::Information)]), None);
        let over = disruption("a", TRAIN, Severity::Blocking, at(3, 6, 0), at(3, 7, 0));
        assert_eq!(on_train(&[over]), None);
        let elsewhere = disruption("a", OTHER, Severity::Blocking, at(3, 7, 0), at(3, 10, 0));
        assert_eq!(on_train(&[elsewhere]), None);
        // Planned works count only when they cut the line, whatever their cause.
        let works = planned("w", Cause::Works, at(3, 0, 0), at(3, 23, 0));
        assert_eq!(on_train(std::slice::from_ref(&works)), None);
        let cut = Disruption {
            severity: Severity::Blocking,
            ..works
        };
        assert_eq!(on_train(&[cut]), Some(Severity::Blocking));
        let days_long = planned("d", Cause::Incident, at(1, 0, 0), at(5, 0, 0));
        assert_eq!(on_train(&[days_long]), None);
    }

    #[test]
    fn a_leg_is_not_disrupted_on_a_guess() {
        // Z to B is the leg's way through A, or another way through Y: the
        // record does not say which.
        let route = |names: &[&str]| Route {
            stops: names
                .iter()
                .map(|name| RouteStop {
                    stop_point: format!("sp:{name}"),
                    stop_area: format!("sa:{name}"),
                    name: (*name).into(),
                    coord: None,
                })
                .collect(),
        };
        let train = WatchedLeg {
            routes: vec![route(&["Z", "A", "B"]), route(&["Z", "Y", "B"])],
            ..leg(TRAIN)
        };
        let place = |name: &str| Place {
            id: format!("sa:{name}"),
            name: name.into(),
        };
        let either = Disruption {
            title: "Incident".into(),
            message: String::new(),
            sections: vec![Section {
                line: TRAIN.into(),
                from: place("Z"),
                to: place("B"),
            }],
            ..incident("a", Severity::Blocking)
        };
        // Nor for a section beyond the leg that only meets it.
        let beyond = Disruption {
            sections: vec![Section {
                line: TRAIN.into(),
                from: place("Z"),
                to: place("A"),
            }],
            ..either.clone()
        };
        assert_eq!(
            severity_on(
                std::slice::from_ref(&beyond),
                at(3, 7, 30),
                &train.leg,
                &train.routes
            ),
            None
        );
        // No backup on a guess...
        assert_eq!(
            severity_on(
                std::slice::from_ref(&either),
                at(3, 7, 30),
                &train.leg,
                &train.routes
            ),
            None
        );
        // ...but notified all the same.
        let commute = Commute {
            outbound: vec![train],
            inbound: vec![],
        };
        let either_scope = Scope::Section {
            from: "Z".into(),
            to: "B".into(),
        };
        assert_eq!(
            decide(
                &mut State::default(),
                at(3, 7, 30),
                MORNING,
                &commute,
                std::slice::from_ref(&either)
            ),
            [Notification::Started {
                line: TRAIN.into(),
                severity: Severity::Blocking,
                disruptions: vec![either],
                scope: either_scope,
            }]
        );
    }

    #[test]
    fn a_notification_says_where_the_shown_record_is_on_the_leg() {
        let route = |names: &[&str]| Route {
            stops: names
                .iter()
                .map(|name| RouteStop {
                    stop_point: format!("sp:{name}"),
                    stop_area: format!("sa:{name}"),
                    name: (*name).into(),
                    coord: None,
                })
                .collect(),
        };
        let train = WatchedLeg {
            routes: vec![route(&["Z", "A", "B"]), route(&["Y", "B"])],
            ..leg(TRAIN)
        };
        let section = |from: &str, to: &str| Section {
            line: TRAIN.into(),
            from: Place {
                id: format!("sa:{from}"),
                name: format!("{from} (Ville)"),
            },
            to: Place {
                id: format!("sa:{to}"),
                name: to.into(),
            },
        };
        // The worse record is shown: its first section is off the leg.
        let records = [
            incident("a", Severity::Disrupted),
            Disruption {
                title: "Incident".into(),
                message: String::new(),
                sections: vec![section("Y", "B"), section("Z", "A")],
                ..incident("b", Severity::Blocking)
            },
        ];
        let commute = Commute {
            outbound: vec![train],
            inbound: vec![],
        };
        assert_eq!(
            decide(
                &mut State::default(),
                at(3, 7, 30),
                MORNING,
                &commute,
                &records,
            ),
            [Notification::Started {
                line: TRAIN.into(),
                severity: Severity::Blocking,
                disruptions: records.to_vec(),
                scope: Scope::Section {
                    from: "Z".into(),
                    to: "A".into()
                },
            }]
        );
    }

    #[test]
    fn a_line_notification_shows_its_worst_record() {
        let records = [
            incident("a", Severity::Disrupted),
            incident("b", Severity::Blocking),
        ];
        assert_eq!(shown(Severity::Blocking, &records), Some(&records[1]));
        // A severity no record has: the first.
        assert_eq!(shown(Severity::Unknown, &records), Some(&records[0]));
        assert_eq!(shown(Severity::Blocking, &[]), None);
    }
}
