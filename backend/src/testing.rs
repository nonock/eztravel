//! What the tests of several modules share.

/// An answer of the API, as `fixtures/api/` fixes it for both sides.
pub(crate) fn fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/../fixtures/api/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Navitia's departures of a stop, by journey, local time, mission and direction.
pub(crate) fn timetable_departures(trains: &[(&str, &str, &str, &str)]) -> serde_json::Value {
    let departures: Vec<_> = trains
        .iter()
        .map(|(journey, time, mission, direction)| serde_json::json!({
            "display_informations": { "headsign": mission, "direction": direction },
            "stop_date_time": { "departure_date_time": time },
            "links": [{ "type": "route", "id": "route:A" }, { "type": "vehicle_journey", "id": journey }]
        }))
        .collect();
    serde_json::json!({ "departures": departures })
}

/// Navitia's arrivals at a stop, by journey and local time.
pub(crate) fn timetable_arrivals(trains: &[(&str, &str)]) -> serde_json::Value {
    let arrivals: Vec<_> = trains
        .iter()
        .map(|(journey, time)| {
            serde_json::json!({
                "display_informations": { "headsign": "X", "direction": "Y" },
                "stop_date_time": { "arrival_date_time": time },
                "links": [{ "type": "vehicle_journey", "id": journey }]
            })
        })
        .collect();
    serde_json::json!({ "arrivals": arrivals })
}
