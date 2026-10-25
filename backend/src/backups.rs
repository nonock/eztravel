//! Rides to take instead of lines of the commute while they are disrupted: the
//! app shows their next departures with those of the way.

use std::collections::BTreeMap;

use jiff::civil::DateTime;
use serde::Deserialize;

use crate::alerts::severity_on;
use crate::calendar::Direction;
use crate::disruptions::{Disruption, Severity};
use crate::relevance::{Leg, Route};

/// Rides that stand in for some lines of one way.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backup {
    pub direction: Direction,
    /// Codes of lines ridden on that way: the backup shows while one of them
    /// is disrupted there.
    pub instead_of: Vec<String>,
    /// In order, as the way's own rides.
    pub rides: Vec<Leg>,
}

/// A backup to show now.
#[derive(Debug, PartialEq, Eq)]
pub struct Needed<'a> {
    pub backup: &'a Backup,
    /// The lines it avoids that are disrupted on the way, and how badly, in its order.
    pub avoided: Vec<(&'a str, Severity)>,
    /// How each of its rides runs: one can be disrupted too.
    pub rides: Vec<Option<Severity>>,
}

/// The backups of the way `direction`, whose rides are `legs`, to show at `now`
/// in network local time: those that avoid a line disrupted on one of the legs,
/// by [`severity_on`].
pub fn needed<'a>(
    backups: &'a [Backup],
    direction: Direction,
    legs: &[Leg],
    disruptions: &[Disruption],
    now: DateTime,
    routes: &BTreeMap<String, Vec<Route>>,
) -> Vec<Needed<'a>> {
    let severity = |leg: &Leg| {
        let line_routes = routes.get(&leg.line).map_or(&[][..], Vec::as_slice);
        severity_on(disruptions, now, leg, line_routes)
    };
    backups
        .iter()
        .filter(|backup| backup.direction == direction)
        .filter_map(|backup| {
            let avoided: Vec<(&str, Severity)> = backup
                .instead_of
                .iter()
                .filter_map(|line| {
                    let worst = legs
                        .iter()
                        .filter(|leg| leg.line == *line)
                        .filter_map(severity)
                        .max_by_key(|severity| *severity == Severity::Blocking)?;
                    Some((line.as_str(), worst))
                })
                .collect();
            (!avoided.is_empty()).then(|| Needed {
                backup,
                avoided,
                rides: backup.rides.iter().map(severity).collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptions::{Cause, Period, Place, Section};
    use crate::relevance::RouteStop;
    use jiff::civil::date;
    use std::sync::LazyLock;

    const TRAIN: &str = "C01740";
    const METRO: &str = "C02000";
    const BACKUP: &str = "C03000";

    fn ride(line: &str, board: &str, alight: &str) -> Leg {
        Leg {
            line: line.into(),
            board: board.into(),
            alight: alight.into(),
            reach: None,
        }
    }

    /// The morning way: the train, then the metro.
    fn morning() -> Vec<Leg> {
        vec![ride(TRAIN, "sa:H", "sa:S"), ride(METRO, "sa:S", "sa:W")]
    }

    /// Instead of the metro in the morning, and instead of either line in the evening.
    fn backups() -> Vec<Backup> {
        vec![
            Backup {
                direction: Direction::Outbound,
                instead_of: vec![METRO.into()],
                rides: vec![ride(BACKUP, "sa:S", "sa:W")],
            },
            Backup {
                direction: Direction::Inbound,
                instead_of: vec![METRO.into(), TRAIN.into()],
                rides: vec![ride(BACKUP, "sa:W", "sa:B"), ride(TRAIN, "sa:B", "sa:H")],
            },
        ]
    }

    static BACKUPS: LazyLock<Vec<Backup>> = LazyLock::new(backups);

    fn now() -> DateTime {
        date(2026, 11, 3).at(8, 0, 0, 0)
    }

    /// Under way on `line` from 7:00 to 10:00 on Tuesday 3 November.
    fn incident(line: &str, severity: Severity) -> Disruption {
        Disruption {
            id: format!("{line}-{severity:?}"),
            lines: vec![line.into()],
            stops: vec![],
            periods: vec![Period {
                begin: date(2026, 11, 3).at(7, 0, 0, 0),
                end: date(2026, 11, 3).at(10, 0, 0, 0),
            }],
            updated: date(2026, 11, 3).at(7, 0, 0, 0),
            cause: Cause::Incident,
            severity,
            title: "Incident".into(),
            short_message: None,
            message: String::new(),
            sections: vec![],
        }
    }

    fn evening() -> Vec<Leg> {
        vec![ride(METRO, "sa:W", "sa:S"), ride(TRAIN, "sa:S", "sa:H")]
    }

    fn needed_at(direction: Direction, disruptions: &[Disruption]) -> Vec<Needed<'static>> {
        let legs = match direction {
            Direction::Outbound => morning(),
            Direction::Inbound => evening(),
        };
        needed(
            &BACKUPS,
            direction,
            &legs,
            disruptions,
            now(),
            &BTreeMap::new(),
        )
    }

    #[test]
    fn no_backup_while_the_way_runs() {
        assert_eq!(needed_at(Direction::Outbound, &[]), []);
        assert_eq!(
            needed_at(
                Direction::Outbound,
                &[incident(TRAIN, Severity::Information)]
            ),
            []
        );
    }

    #[test]
    fn a_backup_shows_while_a_line_it_avoids_is_disrupted_on_its_way() {
        let shown = needed_at(Direction::Outbound, &[incident(METRO, Severity::Blocking)]);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].backup, &backups()[0]);
        assert_eq!(shown[0].avoided, [(METRO, Severity::Blocking)]);
        assert_eq!(shown[0].rides, [None]);
        // Not for another line of the way.
        assert_eq!(
            needed_at(Direction::Outbound, &[incident(TRAIN, Severity::Blocking)]),
            []
        );
    }

    #[test]
    fn each_line_avoided_and_each_ride_says_how_it_runs() {
        let shown = needed_at(
            Direction::Inbound,
            &[
                incident(TRAIN, Severity::Disrupted),
                incident(METRO, Severity::Unknown),
            ],
        );
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].backup, &backups()[1]);
        // In the backup's order.
        assert_eq!(
            shown[0].avoided,
            [(METRO, Severity::Disrupted), (TRAIN, Severity::Disrupted)]
        );
        // Its last ride is on the disrupted train line.
        assert_eq!(shown[0].rides, [None, Some(Severity::Disrupted)]);
    }

    #[test]
    fn a_disruption_off_the_leg_avoided_shows_no_backup() {
        let stop = |id: &str| RouteStop {
            stop_point: format!("sp:{id}"),
            stop_area: format!("sa:{id}"),
            name: id.into(),
            coord: None,
        };
        let routes = BTreeMap::from([(
            METRO.to_string(),
            vec![Route {
                stops: vec![stop("S"), stop("W"), stop("F"), stop("G")],
            }],
        )]);
        let place = |id: &str| Place {
            id: format!("sa:{id}"),
            name: id.into(),
        };
        let beyond = Disruption {
            sections: vec![Section {
                line: METRO.into(),
                from: place("F"),
                to: place("G"),
            }],
            ..incident(METRO, Severity::Blocking)
        };
        let backups = backups();
        let needed_with = |disruption: &Disruption| {
            needed(
                &backups,
                Direction::Outbound,
                &morning(),
                std::slice::from_ref(disruption),
                now(),
                &routes,
            )
            .len()
        };
        assert_eq!(needed_with(&beyond), 0);
        let on_leg = Disruption {
            sections: vec![Section {
                line: METRO.into(),
                from: place("S"),
                to: place("W"),
            }],
            ..beyond
        };
        assert_eq!(needed_with(&on_leg), 1);
    }
}
