//! Client for the PRIM APIs of Île-de-France Mobilités.

use std::time::Duration;

use jiff::civil::DateTime;
use serde::Deserialize;

use crate::departures::{self, Visit};
use crate::disruptions::{Disruption, local_time, parse};
use crate::http;
use crate::relevance::{Route, parse_route};

#[derive(Clone)]
pub struct Prim {
    http: reqwest::Client,
    base_url: String,
    key: String,
}

/// A stop's visits, and how many requests the daily quota still allows.
pub struct Visits {
    pub visits: Vec<Visit>,
    pub remaining: Option<u32>,
}

#[derive(Debug)]
pub enum PrimError {
    Http(reqwest::Error),
    Status(u16),
    Format(serde_json::Error),
}

impl Prim {
    /// `base_url` is `https://prim.iledefrance-mobilites.fr/marketplace`, or a test server.
    pub fn new(base_url: &str, key: &str) -> Self {
        Self::with_timeout(base_url, key, http::TIMEOUT)
    }

    pub(crate) fn with_timeout(base_url: &str, key: &str, timeout: Duration) -> Self {
        Self {
            http: http::client(timeout),
            base_url: base_url.trim_end_matches('/').to_string(),
            key: key.to_string(),
        }
    }

    /// The network's current disruptions attached to one of `lines` (line codes).
    pub async fn disruptions(&self, lines: &[&str]) -> Result<Vec<Disruption>, PrimError> {
        let body = self.get("/disruptions_bulk/disruptions/v2").await?;
        parse(&body, lines).map_err(PrimError::Format)
    }

    /// The calls of the coming hours at a stop (`STIF:StopArea:SP:…:` or `STIF:StopPoint:Q:…:`).
    pub async fn stop_visits(&self, monitoring_ref: &str) -> Result<Visits, PrimError> {
        let (body, remaining) = self
            .counted_get(&format!("/stop-monitoring?MonitoringRef={monitoring_ref}"))
            .await?;
        let visits = departures::parse(&body).map_err(PrimError::Format)?;
        Ok(Visits { visits, remaining })
    }

    /// The routes of a line, with their stops in travel order, as run from `from`.
    ///
    /// All or nothing: on any failure the caller keeps its previous routes.
    pub async fn routes(&self, line: &str, from: DateTime) -> Result<Vec<Route>, PrimError> {
        let body = self
            .get(&format!(
                "/v2/navitia/lines/{}/routes?count=100&depth=0",
                encode(&format!("line:IDFM:{line}"))
            ))
            .await?;
        let list: RouteList = serde_json::from_slice(&body).map_err(PrimError::Format)?;
        let mut routes = vec![];
        for route in list.routes {
            // Enough trips of a weekday morning to see every stop of the route.
            let path = format!(
                "/v2/navitia/routes/{}/route_schedules?items_per_schedule=30&depth=1&data_freshness=base_schedule&from_datetime={}",
                encode(&route.id),
                from.strftime("%Y%m%dT%H%M%S")
            );
            routes.push(parse_route(&self.get(&path).await?).map_err(PrimError::Format)?);
        }
        Ok(routes)
    }

    /// The trains of `line` due to leave `board` during `window`, local time, by
    /// the timetable (Navitia's base schedule), that call at `alight` after it.
    pub async fn timetable(
        &self,
        line: &str,
        board: &str,
        alight: &str,
        window: (DateTime, DateTime),
    ) -> Result<Vec<Timetabled>, PrimError> {
        let (from, until) = window;
        let line = encode(&format!("line:IDFM:{line}"));
        let query = |kind: &str, area: &str| {
            format!(
                "/v2/navitia/stop_areas/{}/lines/{line}/{kind}?from_datetime={}&count=200&depth=1&data_freshness=base_schedule",
                encode(area),
                from.strftime("%Y%m%dT%H%M%S")
            )
        };
        let leaving: Calls = serde_json::from_slice(&self.get(&query("departures", board)).await?)
            .map_err(PrimError::Format)?;
        let arriving: Calls = serde_json::from_slice(&self.get(&query("arrivals", alight)).await?)
            .map_err(PrimError::Format)?;
        Ok(leaving
            .departures
            .into_iter()
            // Navitia gives more than the window: what is beyond is left out.
            .filter(|call| from <= call.time.departure && call.time.departure < until)
            .filter_map(|call| {
                let arrival = arriving
                    .arrivals
                    .iter()
                    .find(|arrival| {
                        arrival.journey() == call.journey()
                            && arrival.time.arrival > call.time.departure
                    })?
                    .time
                    .arrival;
                // "Gare Centrale (Paris)": its town is left out, as live lists do.
                let destination = call.informations.direction;
                let destination = match destination.rsplit_once(" (") {
                    Some((name, town)) if town.ends_with(')') => name.to_string(),
                    _ => destination,
                };
                Some(Timetabled {
                    aimed: call.time.departure,
                    arrival,
                    mission: call.informations.headsign,
                    destination,
                })
            })
            .collect())
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>, PrimError> {
        Ok(self.counted_get(path).await?.0)
    }

    /// The body, and the requests the daily quota still allows when PRIM says.
    async fn counted_get(&self, path: &str) -> Result<(Vec<u8>, Option<u32>), PrimError> {
        let response = self
            .http
            .get(format!("{}{path}", self.base_url))
            .header("apikey", &self.key)
            .send()
            .await
            .map_err(PrimError::Http)?;
        if !response.status().is_success() {
            return Err(PrimError::Status(response.status().as_u16()));
        }
        let remaining = response
            .headers()
            .get("x-ratelimit-remaining-day")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        // A body cut short does not parse: it still ends as an error, never as an empty list.
        let body = response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .unwrap_or_default();
        Ok((body, remaining))
    }
}

/// A train of the timetable, at the stop it leaves from, and its arrival at
/// the alight stop: local times.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timetabled {
    pub aimed: DateTime,
    pub arrival: DateTime,
    pub mission: Option<String>,
    pub destination: String,
}

/// Navitia's departures or arrivals of a stop.
#[derive(Deserialize)]
struct Calls {
    #[serde(default)]
    departures: Vec<Call>,
    #[serde(default)]
    arrivals: Vec<Call>,
}

#[derive(Deserialize)]
struct Call {
    #[serde(rename = "display_informations", default)]
    informations: Informations,
    #[serde(rename = "stop_date_time")]
    time: CallTime,
    links: Vec<CallLink>,
}

impl Call {
    fn journey(&self) -> Option<&str> {
        self.links
            .iter()
            .find(|link| link.kind == "vehicle_journey")
            .map(|link| link.id.as_str())
    }
}

#[derive(Default, Deserialize)]
struct Informations {
    headsign: Option<String>,
    #[serde(default)]
    direction: String,
}

#[derive(Deserialize)]
struct CallTime {
    #[serde(
        rename = "departure_date_time",
        deserialize_with = "local_time",
        default = "never"
    )]
    departure: DateTime,
    #[serde(
        rename = "arrival_date_time",
        deserialize_with = "local_time",
        default = "never"
    )]
    arrival: DateTime,
}

/// A departure without its time, or an arrival without its own: never in a window.
fn never() -> DateTime {
    DateTime::MIN
}

#[derive(Deserialize)]
struct CallLink {
    #[serde(rename = "type")]
    kind: String,
    id: String,
}

/// Navitia ids hold colons, encoded in paths.
fn encode(id: &str) -> String {
    id.replace(':', "%3A")
}

#[derive(Deserialize)]
struct RouteList {
    routes: Vec<RouteId>,
}

#[derive(Deserialize)]
struct RouteId {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{timetable_arrivals, timetable_departures};
    use jiff::civil::date;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "test-key";

    fn feed() -> serde_json::Value {
        serde_json::json!({
            "lastUpdatedDate": "2026-11-03T06:02:15.628Z",
            "disruptions": [{
                "id": "d1",
                "applicationPeriods": [{ "begin": "20261103T064433", "end": "20261103T083000" }],
                "lastUpdate": "20261103T070243",
                "cause": "PERTURBATION",
                "severity": "PERTURBEE",
                "title": "Ligne L : perturbations",
                "message": "<p>Trafic perturbé.</p>"
            }],
            "lines": [{
                "id": "line:IDFM:C01740", "name": "L",
                "impactedObjects": [{ "type": "line", "id": "line:IDFM:C01740", "name": "L", "disruptionIds": ["d1"] }]
            }]
        })
    }

    fn schedule(stops: &[&str]) -> serde_json::Value {
        let rows: Vec<_> = stops
            .iter()
            .map(|name| serde_json::json!({ "stop_point": { "id": format!("sp:{name}"), "name": name, "stop_area": { "id": format!("sa:{name}") } } }))
            .collect();
        serde_json::json!({ "route_schedules": [{ "table": { "rows": rows } }] })
    }

    fn outcome<T>(result: Result<T, PrimError>) -> String {
        match result {
            Ok(_) => "ok".into(),
            Err(PrimError::Http(_)) => "http".into(),
            Err(PrimError::Format(_)) => "format".into(),
            Err(error) => format!("{error:?}"),
        }
    }

    #[tokio::test]
    async fn reads_the_disruptions_of_the_watched_lines() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/disruptions_bulk/disruptions/v2"))
            .and(header("apikey", KEY))
            .respond_with(ResponseTemplate::new(200).set_body_json(feed()))
            .expect(2)
            .mount(&server)
            .await;
        let prim = Prim::new(&server.uri(), KEY);
        let disruptions = prim.disruptions(&["C01740"]).await.unwrap();
        assert_eq!(
            disruptions
                .iter()
                .map(|d| d.id.as_str())
                .collect::<Vec<_>>(),
            ["d1"]
        );
        assert_eq!(outcome(prim.disruptions(&["C02000"]).await), "ok");
    }

    #[tokio::test]
    async fn a_failed_request_is_an_error_not_an_empty_list() {
        // An empty list would read as "the disruption is over".
        let server = MockServer::start().await;
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert_eq!(
            outcome(Prim::new(&server.uri(), KEY).disruptions(&["C01740"]).await),
            "Status(503)"
        );

        let server = MockServer::start().await;
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&server)
            .await;
        assert_eq!(
            outcome(Prim::new(&server.uri(), KEY).disruptions(&["C01740"]).await),
            "format"
        );

        assert_eq!(
            outcome(
                Prim::new("http://127.0.0.1:9", KEY)
                    .disruptions(&["C01740"])
                    .await
            ),
            "http"
        );
    }

    #[tokio::test]
    async fn a_request_that_hangs_is_an_error() {
        // Waiting for ever would stop the poll loop without a word.
        let server = MockServer::start().await;
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(feed())
                    .set_delay(std::time::Duration::from_secs(1)),
            )
            .mount(&server)
            .await;
        let prim = Prim::with_timeout(&server.uri(), KEY, std::time::Duration::from_millis(100));
        assert_eq!(outcome(prim.disruptions(&["C01740"]).await), "http");
    }

    fn stop_answer() -> serde_json::Value {
        serde_json::json!({ "Siri": { "ServiceDelivery": { "StopMonitoringDelivery": [{ "MonitoredStopVisit": [{
            "RecordedAtTime": "2026-11-03T06:48:00.000Z",
            "MonitoredVehicleJourney": {
                "LineRef": { "value": "STIF:Line::C01740:" },
                "FramedVehicleJourneyRef": { "DatedVehicleJourneyRef": "j1" },
                "DestinationName": [{ "value": "Gare Centrale" }],
                "MonitoredCall": { "ExpectedDepartureTime": "2026-11-03T06:52:00.000Z", "DepartureStatus": "onTime" }
            }
        }] }] } } })
    }

    #[tokio::test]
    async fn reads_the_visits_of_a_stop_and_the_quota_left() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/stop-monitoring"))
            .and(query_param("MonitoringRef", "STIF:StopArea:SP:100:"))
            .and(header("apikey", KEY))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(stop_answer())
                    .insert_header("x-ratelimit-remaining-day", "994"),
            )
            .mount(&server)
            .await;
        Mock::given(path("/stop-monitoring"))
            .and(query_param("MonitoringRef", "STIF:StopPoint:Q:300:"))
            .respond_with(ResponseTemplate::new(200).set_body_json(stop_answer()))
            .mount(&server)
            .await;
        let prim = Prim::new(&server.uri(), KEY);
        let station = prim.stop_visits("STIF:StopArea:SP:100:").await.unwrap();
        assert_eq!(
            station
                .visits
                .iter()
                .map(|v| v.journey.as_str())
                .collect::<Vec<_>>(),
            ["j1"]
        );
        assert_eq!(station.remaining, Some(994));
        let platform = prim.stop_visits("STIF:StopPoint:Q:300:").await.unwrap();
        assert_eq!(platform.remaining, None);
    }

    #[tokio::test]
    async fn a_stop_that_cannot_be_read_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(path("/stop-monitoring"))
            .and(query_param("MonitoringRef", "STIF:StopArea:SP:100:"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;
        let prim = Prim::new(&server.uri(), KEY);
        assert_eq!(
            outcome(prim.stop_visits("STIF:StopArea:SP:100:").await),
            "format"
        );
        // No mock for this one: a 404.
        assert_eq!(
            outcome(prim.stop_visits("STIF:StopArea:SP:200:").await),
            "Status(404)"
        );
    }

    async fn navitia() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/navitia/lines/line%3AIDFM%3AC01740/routes"))
            .and(header("apikey", KEY))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "routes": [{ "id": "route:IDFM:A", "name": "F - P" }, { "id": "route:IDFM:B", "name": "P - F" }]
            })))
            .mount(&server)
            .await;
        for (id, stops) in [
            ("route%3AIDFM%3AA", ["F", "N1", "P"]),
            ("route%3AIDFM%3AB", ["P", "N1", "F"]),
        ] {
            Mock::given(method("GET"))
                .and(path(format!("/v2/navitia/routes/{id}/route_schedules")))
                .and(header("apikey", KEY))
                .and(query_param("from_datetime", "20261102T080000"))
                .and(query_param("items_per_schedule", "30"))
                .and(query_param("data_freshness", "base_schedule"))
                .respond_with(ResponseTemplate::new(200).set_body_json(schedule(&stops)))
                .mount(&server)
                .await;
        }
        server
    }

    #[tokio::test]
    async fn the_timetable_keeps_the_trains_that_call_at_the_alight_stop_after() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/navitia/stop_areas/stop_area%3AIDFM%3A71100/lines/line%3AIDFM%3AC01740/departures"))
            .and(query_param("data_freshness", "base_schedule"))
            .and(query_param("from_datetime", "20261103T064500"))
            .respond_with(ResponseTemplate::new(200).set_body_json(timetable_departures(&[
                ("vj:a", "20261103T075240", "PALO", "Gare Centrale (Paris)"),
                // Another branch: it never reaches the alight stop.
                ("vj:b", "20261103T080000", "POLA", "Les Tilleuls"),
                // Named without its town already.
                ("vj:c", "20261103T081000", "PILO", "Gare Centrale"),
                // After the window: Navitia gives more than asked.
                ("vj:d", "20261103T082000", "PALO", "Gare Centrale (Paris)"),
            ])))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v2/navitia/stop_areas/stop_area%3AIDFM%3A71370/lines/line%3AIDFM%3AC01740/arrivals"))
            .and(query_param("data_freshness", "base_schedule"))
            .respond_with(ResponseTemplate::new(200).set_body_json(timetable_arrivals(&[
                ("vj:a", "20261103T080400"),
                ("vj:c", "20261103T082200"),
                ("vj:d", "20261103T083200"),
                // The other way: it passes the alight stop first.
                ("vj:b", "20261103T074000"),
            ])))
            .expect(1)
            .mount(&server)
            .await;
        let prim = Prim::new(&server.uri(), KEY);
        let window = (
            date(2026, 11, 3).at(6, 45, 0, 0),
            date(2026, 11, 3).at(8, 15, 0, 0),
        );
        let trains = prim
            .timetable(
                "C01740",
                "stop_area:IDFM:71100",
                "stop_area:IDFM:71370",
                window,
            )
            .await
            .unwrap();
        assert_eq!(
            trains,
            [
                Timetabled {
                    aimed: date(2026, 11, 3).at(7, 52, 40, 0),
                    arrival: date(2026, 11, 3).at(8, 4, 0, 0),
                    mission: Some("PALO".into()),
                    destination: "Gare Centrale".into(),
                },
                Timetabled {
                    aimed: date(2026, 11, 3).at(8, 10, 0, 0),
                    arrival: date(2026, 11, 3).at(8, 22, 0, 0),
                    mission: Some("PILO".into()),
                    destination: "Gare Centrale".into(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_timetable_that_cannot_be_read_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(path(
            "/v2/navitia/stop_areas/stop_area%3AIDFM%3A71100/lines/line%3AIDFM%3AC01740/departures",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("{"))
        .mount(&server)
        .await;
        let prim = Prim::new(&server.uri(), KEY);
        let window = (
            date(2026, 11, 3).at(6, 45, 0, 0),
            date(2026, 11, 3).at(8, 15, 0, 0),
        );
        let read = prim
            .timetable(
                "C01740",
                "stop_area:IDFM:71100",
                "stop_area:IDFM:71370",
                window,
            )
            .await;
        assert_eq!(outcome(read), "format");
        // The arrivals too: unreadable, or not answered.
        server.reset().await;
        Mock::given(path(
            "/v2/navitia/stop_areas/stop_area%3AIDFM%3A71100/lines/line%3AIDFM%3AC01740/departures",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(timetable_departures(&[])))
        .mount(&server)
        .await;
        let arrivals =
            "/v2/navitia/stop_areas/stop_area%3AIDFM%3A71370/lines/line%3AIDFM%3AC01740/arrivals";
        Mock::given(path(arrivals))
            .respond_with(ResponseTemplate::new(200).set_body_string("{"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(path(arrivals))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        for expected in ["format", "Status(503)"] {
            let read = prim
                .timetable(
                    "C01740",
                    "stop_area:IDFM:71100",
                    "stop_area:IDFM:71370",
                    window,
                )
                .await;
            assert_eq!(outcome(read), expected);
        }
    }

    #[tokio::test]
    async fn reads_each_route_of_a_line_in_order() {
        let server = navitia().await;
        let routes = Prim::new(&server.uri(), KEY)
            .routes("C01740", date(2026, 11, 2).at(8, 0, 0, 0))
            .await
            .unwrap();
        let names: Vec<Vec<&str>> = routes
            .iter()
            .map(|r| r.stops.iter().map(|s| s.name.as_str()).collect())
            .collect();
        assert_eq!(names, [["F", "N1", "P"], ["P", "N1", "F"]]);
        assert_eq!(routes[0].stops[1].stop_area, "sa:N1");
    }

    #[tokio::test]
    async fn one_failed_route_fails_them_all() {
        let server = navitia().await;
        // No schedule mocked for this date: every schedule request gets a 404.
        let result = Prim::new(&server.uri(), KEY)
            .routes("C01740", date(2026, 11, 3).at(8, 0, 0, 0))
            .await;
        assert_eq!(outcome(result), "Status(404)");
        let unknown_line = Prim::new(&server.uri(), KEY)
            .routes("C09999", date(2026, 11, 2).at(8, 0, 0, 0))
            .await;
        assert_eq!(outcome(unknown_line), "Status(404)");
    }

    #[tokio::test]
    async fn an_unreadable_schedule_is_a_format_error() {
        let server = MockServer::start().await;
        Mock::given(path("/v2/navitia/lines/line%3AIDFM%3AC01740/routes"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "routes": [{ "id": "route:IDFM:A" }] })),
            )
            .mount(&server)
            .await;
        Mock::given(path("/v2/navitia/routes/route%3AIDFM%3AA/route_schedules"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("{\"error\": \"no solution\"}"),
            )
            .mount(&server)
            .await;
        assert_eq!(
            outcome(
                Prim::new(&server.uri(), KEY)
                    .routes("C01740", date(2026, 11, 2).at(8, 0, 0, 0))
                    .await
            ),
            "format"
        );
    }

    #[tokio::test]
    async fn an_unreadable_route_list_is_a_format_error() {
        let server = MockServer::start().await;
        Mock::given(path("/v2/navitia/lines/line%3AIDFM%3AC01740/routes"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;
        assert_eq!(
            outcome(
                Prim::new(&server.uri(), KEY)
                    .routes("C01740", date(2026, 11, 2).at(8, 0, 0, 0))
                    .await
            ),
            "format"
        );
    }
}
