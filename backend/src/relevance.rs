//! Whether a disruption affects one leg of the commute.
//!
//! The end stops of a disrupted section are not enough: a branch can share
//! both of them with another branch, and a section that ends where the leg
//! starts still stops its trains. So sections are placed on the routes of the
//! line, whose stops come in travel order from Navitia (`route_schedules`).
//! When the ends of a section fit both the leg's way and another one, a stop
//! named in the disruption's text, on one of the ways only, tells which. A
//! section that only meets the leg at one of its ends is told apart: it stops
//! some of the leg's trains, not the leg.

use std::cmp::Reverse;
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use crate::disruptions::{Disruption, Section};
use crate::push;

/// One route of a line, its stops in travel order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub stops: Vec<RouteStop>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteStop {
    pub stop_point: String,
    pub stop_area: String,
    pub name: String,
    /// None in the routes saved before, until read again.
    #[serde(default)]
    pub coord: Option<Coord>,
}

/// Where a stop is, in decimal degrees, as Navitia writes them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coord {
    pub lat: String,
    pub lon: String,
}

/// A ride on one line, between two stop areas.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leg {
    pub line: String,
    pub board: String,
    pub alight: String,
    /// Minutes from leaving home, or work, to the platform of this ride.
    #[serde(default)]
    pub reach: Option<u32>,
}

impl Route {
    pub(crate) fn area_index(&self, stop_area: &str) -> Option<usize> {
        self.stops
            .iter()
            .position(|stop| stop.stop_area == stop_area)
    }

    fn point_index(&self, stop_point: &str) -> Option<usize> {
        self.stops
            .iter()
            .position(|stop| stop.stop_point == stop_point)
    }
}

/// Reads a Navitia `route_schedules` response.
pub fn parse_route(json: &[u8]) -> Result<Route, serde_json::Error> {
    let response: Response = serde_json::from_slice(json)?;
    let stops = response
        .route_schedules
        .into_iter()
        .next()
        .map(|schedule| schedule.table.rows)
        .unwrap_or_default()
        .into_iter()
        .map(|row| RouteStop {
            stop_point: row.stop_point.id,
            stop_area: row.stop_point.stop_area.id,
            name: row.stop_point.name,
            coord: row.stop_point.coord,
        })
        .collect();
    Ok(Route { stops })
}

/// How a disruption bears on a leg.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bearing {
    Off,
    /// One of its sections meets the leg at one end only, from beyond:
    /// trains coming from, or going into, the section stop, but the leg's
    /// own stretch may still be run.
    Edge,
    /// One of its sections may be the leg's way, or another way between the
    /// same ends that misses the leg, and its text does not name a stop that
    /// tells which.
    Unsure,
    On,
}

/// Whether a disruption may affect the leg: better an alert too many than
/// one missed. `routes` are the routes of the leg's line.
pub fn affects(disruption: &Disruption, leg: &Leg, routes: &[Route]) -> bool {
    bearing(disruption, leg, routes) != Bearing::Off
}

/// `routes` are the routes of the leg's line.
pub fn bearing(disruption: &Disruption, leg: &Leg, routes: &[Route]) -> Bearing {
    if !disruption.lines.contains(&leg.line) {
        return Bearing::Off;
    }
    let spans = spans(leg, routes);
    if spans.is_empty() {
        // Unknown layout: better an alert too many than one missed.
        return Bearing::On;
    }
    let sections: Vec<_> = disruption
        .sections
        .iter()
        .filter(|section| section.line == leg.line)
        .collect();
    if !sections.is_empty() {
        let text = text(disruption);
        return sections
            .iter()
            .map(|section| section_bearing(section, &spans, routes, &text))
            .max()
            .unwrap_or(Bearing::Off);
    }
    if !disruption.stops.is_empty() {
        let on = spans.iter().any(|(route, span)| {
            disruption.stops.iter().any(|stop| {
                route
                    .point_index(&stop.id)
                    .is_some_and(|index| span.contains(&index))
            })
        });
        return if on { Bearing::On } else { Bearing::Off };
    }
    Bearing::On
}

/// Where a disruption is, as told in a notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The whole line, as far as the disruption says.
    Line,
    Section {
        from: String,
        to: String,
    },
    /// Its stops, the first of them and how many.
    Stops {
        first: String,
        count: usize,
    },
}

/// Where `disruption` is, for the leg: its section on the leg's line that
/// bears most on the leg, else its stops, else the whole line. Stops are named
/// without their town. `routes` are the routes of the leg's line.
pub fn scope(disruption: &Disruption, leg: &Leg, routes: &[Route]) -> Scope {
    let spans = spans(leg, routes);
    let text = text(disruption);
    let most = disruption
        .sections
        .iter()
        .filter(|section| section.line == leg.line)
        .enumerate()
        // The first listed among equals.
        .max_by_key(|(index, section)| {
            (
                section_bearing(section, &spans, routes, &text),
                Reverse(*index),
            )
        });
    match (most, disruption.stops.as_slice()) {
        (Some((_, section)), _) => Scope::Section {
            from: town_less(&section.from.name),
            to: town_less(&section.to.name),
        },
        (None, [first, ..]) => Scope::Stops {
            first: town_less(&first.name),
            count: disruption.stops.len(),
        },
        (None, []) => Scope::Line,
    }
}

/// "Gare Centrale (Ville)" -> "Gare Centrale", as PRIM names a section's ends.
fn town_less(name: &str) -> String {
    match name.rsplit_once(" (") {
        Some((stop, town)) if town.ends_with(')') => stop.to_string(),
        _ => name.to_string(),
    }
}

/// The routes that serve the leg, with the leg's position on each.
fn spans<'a>(leg: &Leg, routes: &'a [Route]) -> Vec<(&'a Route, RangeInclusive<usize>)> {
    routes
        .iter()
        .filter_map(|route| {
            let board = route.area_index(&leg.board)?;
            let alight = route.area_index(&leg.alight)?;
            (board < alight).then_some((route, board..=alight))
        })
        .collect()
}

/// The disruption's title and texts, by [`words`].
fn text(disruption: &Disruption) -> String {
    words(&format!(
        "{} {} {}",
        disruption.title,
        disruption.short_message.as_deref().unwrap_or_default(),
        push::plain_text(&disruption.message)
    ))
}

/// How `section` bears on the leg at any of its `spans`.
fn section_bearing(
    section: &Section,
    spans: &[(&Route, RangeInclusive<usize>)],
    routes: &[Route],
    text: &str,
) -> Bearing {
    spans
        .iter()
        .map(|(route, span)| on_span(section, route, span, routes, text))
        .max()
        .unwrap_or(Bearing::Off)
}

/// How `section` bears on the leg at `span` of `route`, one of `routes`;
/// `text` is the disruption's, by [`words`].
fn on_span(
    section: &Section,
    route: &Route,
    span: &RangeInclusive<usize>,
    routes: &[Route],
    text: &str,
) -> Bearing {
    let (Some(from), Some(to)) = (
        route.area_index(&section.from.id),
        route.area_index(&section.to.id),
    ) else {
        return Bearing::Off;
    };
    let (first, last) = (from.min(to), from.max(to));
    // Touching the leg counts: trains cannot come from, or go into, the section.
    if first > *span.end() || last < *span.start() {
        return Bearing::Off;
    }
    let way = &route.stops[first..=last];
    let covered = &route.stops[first.max(*span.start())..=last.min(*span.end())];
    if covered.len() == 1 && way.len() > 1 {
        return Bearing::Edge;
    }
    let passes = |way: &[RouteStop], area: &str| way.iter().any(|stop| stop.stop_area == area);
    // Other ways between the same ends that miss some of the leg.
    let others: Vec<&[RouteStop]> = routes
        .iter()
        .filter_map(|other| {
            let from = other.area_index(&section.from.id)?;
            let to = other.area_index(&section.to.id)?;
            let other_way = &other.stops[from.min(to)..=from.max(to)];
            (!covered
                .iter()
                .all(|stop| passes(other_way, &stop.stop_area)))
            .then_some(other_way)
        })
        .collect();
    if others.is_empty() {
        return Bearing::On;
    }
    // A stop the text names on one of the ways only tells which is meant.
    let named = |stop: &&RouteStop| text.contains(&words(&stop.name));
    if way
        .iter()
        .filter(named)
        .any(|stop| !others.iter().any(|other| passes(other, &stop.stop_area)))
    {
        return Bearing::On;
    }
    if others
        .iter()
        .flat_map(|other| other.iter())
        .filter(named)
        .any(|stop| !passes(way, &stop.stop_area))
    {
        return Bearing::Off;
    }
    Bearing::Unsure
}

/// The words of `text` in lower case, each between spaces, so that a name is
/// found whole: " pont d en haut ".
fn words(text: &str) -> String {
    let mut words = String::from(" ");
    for word in text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        words.push_str(&word.to_lowercase());
        words.push(' ');
    }
    words
}

#[derive(Deserialize)]
struct Response {
    route_schedules: Vec<Schedule>,
}

#[derive(Deserialize)]
struct Schedule {
    table: Table,
}

#[derive(Deserialize)]
struct Table {
    rows: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    stop_point: NavitiaStop,
}

#[derive(Deserialize)]
struct NavitiaStop {
    id: String,
    name: String,
    stop_area: NavitiaArea,
    #[serde(default)]
    coord: Option<Coord>,
}

#[derive(Deserialize)]
struct NavitiaArea {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptions::{Cause, Place, Section, Severity};
    use jiff::civil::date;

    const LINE: &str = "C01740";

    // An example branching line. Two branches leave the far terminus F and
    // meet at T2 before the city terminus P:
    //   north branch: F - N2 - N1 - T1 - T2 - P
    //   west branch:  F - W1 - T2 - P
    fn route(names: &[&str]) -> Route {
        Route {
            stops: names
                .iter()
                .map(|name| RouteStop {
                    stop_point: format!("sp:{name}"),
                    stop_area: format!("sa:{name}"),
                    name: name.to_string(),
                    coord: None,
                })
                .collect(),
        }
    }

    fn routes() -> Vec<Route> {
        vec![
            route(&["F", "N2", "N1", "T1", "T2", "P"]),
            route(&["P", "T2", "T1", "N1", "N2", "F"]),
            route(&["F", "W1", "T2", "P"]),
            route(&["P", "T2", "W1", "F"]),
        ]
    }

    fn leg(board: &str, alight: &str) -> Leg {
        Leg {
            line: LINE.into(),
            board: format!("sa:{board}"),
            alight: format!("sa:{alight}"),
            reach: None,
        }
    }

    fn place(name: &str, kind: &str) -> Place {
        Place {
            id: format!("{kind}:{name}"),
            name: name.into(),
        }
    }

    fn section(from: &str, to: &str) -> Section {
        Section {
            line: LINE.into(),
            from: place(from, "sa"),
            to: place(to, "sa"),
        }
    }

    fn disruption(sections: Vec<Section>, stops: Vec<Place>) -> Disruption {
        Disruption {
            id: "d".into(),
            lines: vec![LINE.into()],
            stops,
            periods: vec![],
            updated: date(2026, 11, 3).at(7, 0, 0, 0),
            cause: Cause::Incident,
            severity: Severity::Disrupted,
            title: "Ligne L : trafic perturbé".into(),
            short_message: None,
            message: String::new(),
            sections,
        }
    }

    fn on_sections(sections: &[(&str, &str)]) -> Disruption {
        disruption(
            sections
                .iter()
                .map(|(from, to)| section(from, to))
                .collect(),
            vec![],
        )
    }

    #[test]
    fn a_section_covering_the_leg() {
        // Both directions are listed, as in the feed.
        let disruption = on_sections(&[("P", "N2"), ("N2", "P")]);
        assert!(affects(&disruption, &leg("N1", "P"), &routes()));
    }

    #[test]
    fn a_section_ending_where_the_leg_starts() {
        // Trains coming from N2 cannot reach N1.
        assert!(affects(
            &on_sections(&[("N1", "N2")]),
            &leg("N1", "P"),
            &routes()
        ));
        // But those starting at N1 may still run.
        assert_eq!(
            bearing(&on_sections(&[("N1", "N2")]), &leg("N1", "P"), &routes()),
            Bearing::Edge
        );
    }

    #[test]
    fn a_section_ending_where_the_leg_ends_on_the_way_back() {
        assert!(affects(
            &on_sections(&[("N2", "N1")]),
            &leg("P", "N1"),
            &routes()
        ));
        assert_eq!(
            bearing(&on_sections(&[("N2", "N1")]), &leg("P", "N1"), &routes()),
            Bearing::Edge
        );
    }

    #[test]
    fn a_closed_stop_at_the_end_of_the_leg_is_on_it() {
        // A section of one stop: the leg cannot be boarded.
        assert_eq!(
            bearing(&on_sections(&[("N1", "N1")]), &leg("N1", "P"), &routes()),
            Bearing::On
        );
    }

    #[test]
    fn a_section_further_up_the_branch_is_left_out() {
        assert!(!affects(
            &on_sections(&[("F", "N2")]),
            &leg("N1", "P"),
            &routes()
        ));
    }

    #[test]
    fn a_section_of_the_other_branch_is_left_out() {
        // F and P belong to both branches, W1 only to the west one.
        assert!(!affects(
            &on_sections(&[("F", "W1")]),
            &leg("N1", "P"),
            &routes()
        ));
        assert!(!affects(
            &on_sections(&[("W1", "P")]),
            &leg("N1", "P"),
            &routes()
        ));
    }

    #[test]
    fn a_section_whose_ends_fit_both_branches_counts() {
        // F to P could be either branch: better an alert too many than one missed.
        assert!(affects(
            &on_sections(&[("F", "P")]),
            &leg("N1", "P"),
            &routes()
        ));
        // But it is only a guess.
        assert_eq!(
            bearing(&on_sections(&[("F", "P")]), &leg("N1", "P"), &routes()),
            Bearing::Unsure
        );
    }

    fn told(sections: &[(&str, &str)], message: &str) -> Disruption {
        Disruption {
            message: message.into(),
            ..on_sections(sections)
        }
    }

    #[test]
    fn a_stop_named_on_one_branch_only_tells_which() {
        // W1 is only on the west branch, N2 only on the north one.
        let to_town = leg("N1", "P");
        assert_eq!(
            bearing(
                &told(&[("F", "P")], "<p>Trains ending at W1 do not run.</p>"),
                &to_town,
                &routes()
            ),
            Bearing::Off
        );
        assert_eq!(
            bearing(
                &told(&[("F", "P")], "Trains calling at N2 do not run."),
                &to_town,
                &routes()
            ),
            Bearing::On
        );
        // On the way back too.
        assert_eq!(
            bearing(&told(&[("P", "F")], "W1"), &leg("P", "N1"), &routes()),
            Bearing::Off
        );
        // The title tells as well.
        let titled = Disruption {
            title: "Suppression W1 <> P".into(),
            ..on_sections(&[("F", "P")])
        };
        assert_eq!(bearing(&titled, &to_town, &routes()), Bearing::Off);
        assert!(!affects(&titled, &to_town, &routes()));
    }

    #[test]
    fn a_stop_on_both_branches_tells_nothing() {
        // F and P are on both, and a longer name is not W1.
        assert_eq!(
            bearing(
                &told(&[("F", "P")], "Between F and P, W10 is closed."),
                &leg("N1", "P"),
                &routes()
            ),
            Bearing::Unsure
        );
    }

    #[test]
    fn a_section_over_the_leg_and_beyond_is_on_it() {
        assert_eq!(
            bearing(&on_sections(&[("N2", "T1")]), &leg("N1", "P"), &routes()),
            Bearing::On
        );
        // The worst section counts.
        assert_eq!(
            bearing(
                &on_sections(&[("N1", "N2"), ("N2", "T1")]),
                &leg("N1", "P"),
                &routes()
            ),
            Bearing::On
        );
    }

    #[test]
    fn a_disruption_is_said_where_it_bears_most_on_the_leg() {
        // The first section is on the other branch, the second on the leg.
        let both = on_sections(&[("F", "W1"), ("W1", "F"), ("N2", "T1"), ("T1", "N2")]);
        assert_eq!(
            scope(&both, &leg("N1", "P"), &routes()),
            Scope::Section {
                from: "N2".into(),
                to: "T1".into()
            }
        );
        // Among equals, the first listed.
        assert_eq!(
            scope(&both, &leg("W1", "P"), &routes()),
            Scope::Section {
                from: "F".into(),
                to: "W1".into()
            }
        );
    }

    #[test]
    fn a_stop_is_named_without_its_town() {
        let mut town = section("N2", "T1");
        town.from.name = "Gare Centrale (Ville)".into();
        town.to.name = "Les Aubépines (Autre Ville)".into();
        assert_eq!(
            scope(&disruption(vec![town], vec![]), &leg("N1", "P"), &routes()),
            Scope::Section {
                from: "Gare Centrale".into(),
                to: "Les Aubépines".into()
            }
        );
    }

    #[test]
    fn without_section_a_disruption_is_said_at_its_stops_or_on_the_line() {
        let at = |stops: &[&str]| {
            scope(
                &disruption(vec![], stops.iter().map(|stop| place(stop, "sp")).collect()),
                &leg("N1", "P"),
                &routes(),
            )
        };
        assert_eq!(
            at(&["T1"]),
            Scope::Stops {
                first: "T1".into(),
                count: 1
            }
        );
        assert_eq!(
            at(&["T1", "T2"]),
            Scope::Stops {
                first: "T1".into(),
                count: 2
            }
        );
        assert_eq!(at(&[]), Scope::Line);
        // Another line's sections say nothing of ours.
        let mut elsewhere = section("F", "W1");
        elsewhere.line = "C02000".into();
        assert_eq!(
            scope(
                &disruption(vec![elsewhere], vec![]),
                &leg("N1", "P"),
                &routes()
            ),
            Scope::Line
        );
    }

    #[test]
    fn a_disruption_without_section_nor_stop_covers_the_whole_line() {
        assert!(affects(
            &disruption(vec![], vec![]),
            &leg("N1", "P"),
            &routes()
        ));
    }

    #[test]
    fn without_section_the_listed_stops_decide() {
        let at_t1 = disruption(vec![], vec![place("T1", "sp")]);
        let at_w1 = disruption(vec![], vec![place("W1", "sp")]);
        assert!(affects(&at_t1, &leg("N1", "P"), &routes()));
        assert!(!affects(&at_w1, &leg("N1", "P"), &routes()));
    }

    #[test]
    fn sections_of_another_line_are_ignored() {
        let mut elsewhere = section("F", "W1");
        elsewhere.line = "C02000".into();
        let disruption = disruption(vec![elsewhere], vec![]);
        // Only the other line's section is known: the disruption covers all of ours.
        assert!(affects(&disruption, &leg("N1", "P"), &routes()));
    }

    #[test]
    fn a_disruption_of_another_line_is_left_out() {
        let mut other = on_sections(&[("N1", "P")]);
        other.lines = vec!["C02000".into()];
        assert!(!affects(&other, &leg("N1", "P"), &routes()));
    }

    #[test]
    fn without_a_route_for_the_leg_every_disruption_counts() {
        // A leg in the wrong direction, or routes not loaded yet.
        assert!(affects(
            &on_sections(&[("F", "W1")]),
            &leg("P", "P"),
            &routes()
        ));
        assert!(affects(&on_sections(&[("F", "W1")]), &leg("N1", "P"), &[]));
    }

    #[test]
    fn reads_the_stops_of_a_route_in_order() {
        let json = serde_json::json!({
            "route_schedules": [{
                "display_informations": { "code": "L", "direction": "P" },
                "table": {
                    "headers": [],
                    "rows": [
                        { "stop_point": { "id": "sp:N1", "name": "N1", "stop_area": { "id": "sa:N1", "name": "N1 (Ville)" } }, "date_times": [] },
                        { "stop_point": { "id": "sp:T1", "name": "T1", "stop_area": { "id": "sa:T1", "name": "T1 (Ville)" } }, "date_times": [] },
                        { "stop_point": { "id": "sp:P", "name": "P", "stop_area": { "id": "sa:P", "name": "P (Ville)" } }, "date_times": [] }
                    ]
                }
            }],
            "disruptions": []
        });
        assert_eq!(
            parse_route(json.to_string().as_bytes()).unwrap(),
            route(&["N1", "T1", "P"])
        );
    }

    #[test]
    fn a_stop_keeps_where_it_is() {
        let json = serde_json::json!({ "route_schedules": [{ "table": { "rows": [
            { "stop_point": { "id": "sp:N1", "name": "N1", "coord": { "lat": "48.876", "lon": "2.325" }, "stop_area": { "id": "sa:N1" } } },
            { "stop_point": { "id": "sp:T1", "name": "T1", "stop_area": { "id": "sa:T1" } } }
        ]}}]});
        let route = parse_route(json.to_string().as_bytes()).unwrap();
        let coord = Coord {
            lat: "48.876".into(),
            lon: "2.325".into(),
        };
        assert_eq!(route.stops[0].coord, Some(coord));
        assert_eq!(route.stops[1].coord, None);
        // Routes saved by the previous version are read without.
        let saved = r#"[{"stops":[{"stop_point":"sp:1","stop_area":"sa:1","name":"One"}]}]"#;
        let saved: Vec<Route> = serde_json::from_str(saved).unwrap();
        assert_eq!(saved[0].stops[0].coord, None);
    }

    #[test]
    fn routes_can_be_kept_between_runs() {
        let saved = serde_json::to_string(&routes()).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<Route>>(&saved).unwrap(),
            routes()
        );
    }

    #[test]
    fn a_route_without_schedule_has_no_stop() {
        assert_eq!(
            parse_route(br#"{"route_schedules": []}"#).unwrap(),
            Route::default()
        );
        assert!(parse_route(b"{").is_err());
    }
}
