//! Client for the PRIM APIs of Île-de-France Mobilités.

use jiff::civil::DateTime;
use serde::Deserialize;

use crate::disruptions::{Disruption, parse};
use crate::relevance::{Route, parse_route};

pub struct Prim {
    http: reqwest::Client,
    base_url: String,
    key: String,
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
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            key: key.to_string(),
        }
    }

    /// The network's current disruptions attached to one of `lines` (line codes).
    pub async fn disruptions(&self, lines: &[&str]) -> Result<Vec<Disruption>, PrimError> {
        let body = self.get("/disruptions_bulk/disruptions/v2").await?;
        parse(&body, lines).map_err(PrimError::Format)
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

    async fn get(&self, path: &str) -> Result<Vec<u8>, PrimError> {
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
        // A body cut short does not parse: it still ends as an error, never as an empty list.
        Ok(response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .unwrap_or_default())
    }
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
