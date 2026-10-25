//! Whether a disruption affects one leg of the commute.
//!
//! The end stops of a disrupted section are not enough: a branch can share
//! both of them with another branch, and a section that ends where the leg
//! starts still stops its trains. So sections are placed on the routes of the
//! line, whose stops come in travel order from Navitia (`route_schedules`).

use serde::{Deserialize, Serialize};

use crate::disruptions::Disruption;

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

/// `routes` are the routes of the leg's line.
pub fn affects(disruption: &Disruption, leg: &Leg, routes: &[Route]) -> bool {
    if !disruption.lines.contains(&leg.line) {
        return false;
    }
    // Routes that serve the leg, with the leg's position on each.
    let spans: Vec<_> = routes
        .iter()
        .filter_map(|route| {
            let board = route.area_index(&leg.board)?;
            let alight = route.area_index(&leg.alight)?;
            (board < alight).then_some((route, board..=alight))
        })
        .collect();
    if spans.is_empty() {
        // Unknown layout: better an alert too many than one missed.
        return true;
    }
    let sections: Vec<_> = disruption
        .sections
        .iter()
        .filter(|section| section.line == leg.line)
        .collect();
    if !sections.is_empty() {
        return spans.iter().any(|(route, span)| {
            sections.iter().any(|section| {
                match (
                    route.area_index(&section.from.id),
                    route.area_index(&section.to.id),
                ) {
                    // Touching the leg counts: trains cannot come from, or go into, the section.
                    (Some(from), Some(to)) => {
                        from.min(to) <= *span.end() && from.max(to) >= *span.start()
                    }
                    _ => false,
                }
            })
        });
    }
    if !disruption.stops.is_empty() {
        return spans.iter().any(|(route, span)| {
            disruption.stops.iter().any(|stop| {
                route
                    .point_index(&stop.id)
                    .is_some_and(|index| span.contains(&index))
            })
        });
    }
    true
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
    }

    #[test]
    fn a_section_ending_where_the_leg_ends_on_the_way_back() {
        assert!(affects(
            &on_sections(&[("N2", "N1")]),
            &leg("P", "N1"),
            &routes()
        ));
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
