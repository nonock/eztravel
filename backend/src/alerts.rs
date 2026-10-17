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

use jiff::SignedDuration;
use jiff::civil::{Date, DateTime};

use crate::calendar::Direction;
use crate::disruptions::{Cause, Disruption, Severity};
use crate::relevance::{Leg, Route, affects};

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
    Started {
        line: String,
        severity: Severity,
        disruptions: Vec<Disruption>,
    },
    Changed {
        line: String,
        from: Severity,
        to: Severity,
        disruptions: Vec<Disruption>,
    },
    Ended {
        line: String,
    },
    Planned {
        disruptions: Vec<Disruption>,
    },
}

/// What has been notified so far. Kept between polls.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    window: Option<(Date, Direction)>,
    lines: Vec<(String, Severity)>,
    planned_day: Option<Date>,
    planned_titles: Vec<String>,
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
        let previous = state
            .lines
            .iter()
            .find(|(known, _)| *known == line)
            .map(|(_, severity)| *severity);
        state.lines.retain(|(known, _)| *known != line);
        if let Some(severity) = severity {
            state.lines.push((line.clone(), severity));
        }
        match (previous, severity) {
            (None, Some(severity)) => {
                notifications.push(Notification::Started {
                    line,
                    severity,
                    disruptions: incidents,
                });
            }
            (Some(from), Some(to)) if from != to => {
                notifications.push(Notification::Changed {
                    line,
                    from,
                    to,
                    disruptions: incidents,
                });
            }
            (Some(_), None) => notifications.push(Notification::Ended { line }),
            _ => {}
        }
    }
    notifications
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
    use crate::disruptions::{Period, Section};
    use jiff::civil::date;

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
                disruptions: worse.to_vec()
            }]
        );
        let better = [incident("c", Severity::Disrupted)];
        assert_eq!(
            poll(&mut state, at(3, 7, 34), MORNING, &better),
            [Notification::Changed {
                line: TRAIN.into(),
                from: Severity::Blocking,
                to: Severity::Disrupted,
                disruptions: better.to_vec()
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
    fn planned_information_only_is_not_announced() {
        let mut state = State::default();
        let notice = Disruption {
            severity: Severity::Information,
            ..planned("i", Cause::Works, at(3, 5, 0), at(3, 23, 0))
        };
        assert_eq!(poll(&mut state, at(3, 7, 30), MORNING, &[notice]), []);
    }
}
