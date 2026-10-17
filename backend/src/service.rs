//! One poll of the service: what to fetch, what to send, what to remember.
//!
//! The alert state is committed only once every notification it decided has
//! been sent, so a failed send is retried at the next poll: a rare duplicate
//! is better than a missed alert. A failed read of PRIM decides nothing,
//! since an empty answer would read as "the disruption is over".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use jiff::civil::Date;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::alerts::{Commute, State, WatchedLeg, decide};
use crate::calendar::watched_direction;
use crate::config::Config;
use crate::prim::Prim;
use crate::push::{self, PushError, Sender};
use crate::relevance::{Leg, Route};

const STATE_FILE: &str = "state.json";
const ROUTES_FILE: &str = "routes.json";

pub struct Service {
    config: Config,
    prim: Prim,
    sender: Sender,
    devices: Vec<String>,
    data_dir: PathBuf,
    state: State,
    routes: BTreeMap<String, Vec<Route>>,
    routes_day: Option<Date>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    /// Not an office day, or outside the watch windows.
    Idle,
    /// PRIM could not be read: nothing decided, nothing sent.
    Unavailable(String),
    /// Every notification decided was sent (possibly none).
    Sent(usize),
    /// A send failed: the state stays as it was, so the next poll tries again.
    NotSent(String),
}

impl Service {
    /// Picks up the state and routes saved in `data_dir` by a previous run, if any.
    pub fn new(
        config: Config,
        prim: Prim,
        sender: Sender,
        devices: Vec<String>,
        data_dir: &Path,
    ) -> Self {
        Self {
            state: load(&data_dir.join(STATE_FILE)).unwrap_or_default(),
            routes: load(&data_dir.join(ROUTES_FILE)).unwrap_or_default(),
            routes_day: None,
            config,
            prim,
            sender,
            devices,
            data_dir: data_dir.to_path_buf(),
        }
    }

    pub async fn poll(&mut self, now: Timestamp) -> Poll {
        let time_zone = self.config.time_zone();
        let local = now.to_zoned(time_zone.clone()).datetime();
        let direction =
            watched_direction(now, &time_zone, &self.config.calendar, &self.config.windows);
        if direction.is_none() {
            let mut next = self.state.clone();
            decide(&mut next, local, None, &self.commute(), &[]);
            self.commit(next);
            return Poll::Idle;
        }
        self.refresh_routes(local.date()).await;
        let lines = self.config.line_codes();
        let codes: Vec<&str> = lines.iter().map(String::as_str).collect();
        let disruptions = match self.prim.disruptions(&codes).await {
            Ok(disruptions) => disruptions,
            Err(error) => return Poll::Unavailable(format!("{error:?}")),
        };
        let mut next = self.state.clone();
        let notifications = decide(&mut next, local, direction, &self.commute(), &disruptions);
        for notification in &notifications {
            let data = push::data(notification, &self.config.lines);
            for device in &self.devices {
                match self.sender.send(now, device, &data).await {
                    Ok(()) => {}
                    Err(PushError::Unregistered) => {
                        eprintln!("Firebase no longer knows a device: the app must register again");
                    }
                    Err(error) => return Poll::NotSent(format!("{error:?}")),
                }
            }
        }
        self.commit(next);
        Poll::Sent(notifications.len())
    }

    fn commute(&self) -> Commute {
        let watched = |legs: &[Leg]| {
            legs.iter()
                .map(|leg| WatchedLeg {
                    leg: leg.clone(),
                    routes: self.routes.get(&leg.line).cloned().unwrap_or_default(),
                })
                .collect()
        };
        Commute {
            outbound: watched(&self.config.outbound),
            inbound: watched(&self.config.inbound),
        }
    }

    fn commit(&mut self, next: State) {
        if next != self.state {
            self.state = next;
            save(&self.data_dir.join(STATE_FILE), &self.state);
        }
    }

    /// Once a day, even after a failure: Navitia's daily quota must last.
    async fn refresh_routes(&mut self, today: Date) {
        if self.routes_day == Some(today) {
            return;
        }
        self.routes_day = Some(today);
        for line in self.config.line_codes() {
            match self.prim.routes(&line, today.at(8, 0, 0, 0)).await {
                Ok(routes) => {
                    self.routes.insert(line, routes);
                }
                Err(error) => {
                    eprintln!("routes of {line} unavailable, keeping the saved ones: {error:?}")
                }
            }
        }
        save(&self.data_dir.join(ROUTES_FILE), &self.routes);
        for leg in self.config.outbound.iter().chain(&self.config.inbound) {
            let has =
                |route: &Route, area: &str| route.stops.iter().any(|stop| stop.stop_area == area);
            let served = self.routes.get(&leg.line).is_some_and(|routes| {
                routes
                    .iter()
                    .any(|route| has(route, &leg.board) && has(route, &leg.alight))
            });
            if !served {
                eprintln!(
                    "no route of {} goes from {} to {}: every disruption of the line will count",
                    leg.line, leg.board, leg.alight
                );
            }
        }
    }
}

fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!("ignoring {}, which cannot be read: {error}", path.display());
            None
        }
    }
}

/// Written whole then renamed, so that a crash never leaves half a file.
fn save<T: Serialize>(path: &Path, value: &T) {
    let draft = path.with_extension("tmp");
    let bytes = serde_json::to_vec(value).expect("states and routes always serialize");
    if let Err(error) = std::fs::write(&draft, bytes).and_then(|()| std::fs::rename(&draft, path)) {
        eprintln!("could not save {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::testing::account;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const EXAMPLE: &str = include_str!("../config.example.toml");
    // Tuesday 3 November 2026, an office day in the example: 7:00 in Paris.
    const MORNING: &str = "2026-11-03T06:00:00Z";
    const TWO_MINUTES_LATER: &str = "2026-11-03T06:02:00Z";
    // 11:00 in Paris, outside the windows.
    const LATE_MORNING: &str = "2026-11-03T10:00:00Z";

    fn at(instant: &str) -> Timestamp {
        instant.parse().unwrap()
    }

    fn feed(disrupted: bool) -> serde_json::Value {
        let ids: Vec<&str> = if disrupted { vec!["d1"] } else { vec![] };
        serde_json::json!({
            "disruptions": [{
                "id": "d1",
                "applicationPeriods": [{ "begin": "20261103T064433", "end": "20261103T093000" }],
                "lastUpdate": "20261103T064500",
                "cause": "PERTURBATION",
                "severity": "BLOQUANTE",
                "title": "Ligne L : trafic interrompu",
                "message": "<p>Le trafic est interrompu.</p>"
            }],
            "lines": [{ "id": "line:IDFM:C01740", "impactedObjects": [{ "type": "line", "id": "line:IDFM:C01740", "name": "L", "disruptionIds": ids }] }]
        })
    }

    struct Setup {
        server: MockServer,
        dir: tempfile::TempDir,
    }

    impl Setup {
        /// Lines without routes, which keeps relevance out of the way.
        async fn new() -> Self {
            let server = MockServer::start().await;
            for line in ["C01740", "C02000"] {
                Mock::given(path(format!(
                    "/v2/navitia/lines/line%3AIDFM%3A{line}/routes"
                )))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({ "routes": [] })),
                )
                .mount(&server)
                .await;
            }
            Self::with(server).await
        }

        async fn with(server: MockServer) -> Self {
            Mock::given(path("/token"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::json!({ "access_token": "a", "expires_in": 3599 }),
                    ),
                )
                .mount(&server)
                .await;
            Setup {
                server,
                dir: tempfile::tempdir().unwrap(),
            }
        }

        async fn prim_answers(&self, status: u16, disrupted: bool, times: u64) {
            Mock::given(method("GET"))
                .and(path("/disruptions_bulk/disruptions/v2"))
                .respond_with(ResponseTemplate::new(status).set_body_json(feed(disrupted)))
                .expect(times)
                .mount(&self.server)
                .await;
        }

        async fn firebase_answers(&self, status: u16, times: u64) {
            let body = if status == 404 { "UNREGISTERED" } else { "" };
            let mock = Mock::given(method("POST"))
                .and(path("/v1/projects/demo-project/messages:send"))
                .and(body_string_contains(r#""type":"started""#))
                .respond_with(ResponseTemplate::new(status).set_body_string(body));
            // Answers only `times` requests, so that a later mock can answer differently.
            let mock = if times > 0 {
                mock.up_to_n_times(times)
            } else {
                mock
            };
            mock.expect(times).mount(&self.server).await;
        }

        fn service(&self) -> Service {
            let uri = self.server.uri();
            Service::new(
                Config::parse(EXAMPLE).unwrap(),
                Prim::new(&uri, "key"),
                Sender::new(account(&format!("{uri}/token")), &uri),
                vec!["phone".into()],
                self.dir.path(),
            )
        }

        fn saved_state(&self) -> Option<String> {
            std::fs::read_to_string(self.dir.path().join("state.json")).ok()
        }
    }

    #[tokio::test]
    async fn an_alert_is_sent_once_and_remembered() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 2).await;
        setup.firebase_answers(200, 1).await;
        let mut service = setup.service();
        assert_eq!(service.poll(at(MORNING)).await, Poll::Sent(1));
        assert!(setup.saved_state().is_some());
        assert_eq!(service.poll(at(TWO_MINUTES_LATER)).await, Poll::Sent(0));
    }

    #[tokio::test]
    async fn outside_the_windows_prim_is_not_called() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 0).await;
        assert_eq!(setup.service().poll(at(LATE_MORNING)).await, Poll::Idle);
    }

    #[tokio::test]
    async fn without_prim_nothing_is_decided() {
        let setup = Setup::new().await;
        setup.prim_answers(503, true, 1).await;
        setup.firebase_answers(200, 0).await;
        let mut service = setup.service();
        assert_eq!(
            service.poll(at(MORNING)).await,
            Poll::Unavailable("Status(503)".into())
        );
        assert_eq!(setup.saved_state(), None);
    }

    #[tokio::test]
    async fn a_failed_send_is_tried_again_at_the_next_poll() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 2).await;
        setup.firebase_answers(500, 1).await;
        let mut service = setup.service();
        assert_eq!(
            service.poll(at(MORNING)).await,
            Poll::NotSent(r#"Status(500, "")"#.into())
        );
        assert_eq!(setup.saved_state(), None);
        setup.firebase_answers(200, 1).await;
        assert_eq!(service.poll(at(TWO_MINUTES_LATER)).await, Poll::Sent(1));
    }

    #[tokio::test]
    async fn a_restart_does_not_send_again() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 2).await;
        setup.firebase_answers(200, 1).await;
        assert_eq!(setup.service().poll(at(MORNING)).await, Poll::Sent(1));
        assert_eq!(
            setup.service().poll(at(TWO_MINUTES_LATER)).await,
            Poll::Sent(0)
        );
    }

    #[tokio::test]
    async fn a_phone_unknown_to_firebase_does_not_hold_alerts_back() {
        // Logged, and the state moves on: retrying would never succeed.
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(404, 1).await;
        assert_eq!(setup.service().poll(at(MORNING)).await, Poll::Sent(1));
        assert!(setup.saved_state().is_some());
    }

    #[tokio::test]
    async fn closing_the_window_is_remembered() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(200, 1).await;
        let mut service = setup.service();
        service.poll(at(MORNING)).await;
        let during = setup.saved_state();
        assert_eq!(service.poll(at(LATE_MORNING)).await, Poll::Idle);
        assert_ne!(setup.saved_state(), during);
    }

    #[tokio::test]
    async fn an_unreadable_saved_state_starts_afresh() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(200, 1).await;
        std::fs::write(setup.dir.path().join("state.json"), "not json").unwrap();
        assert_eq!(setup.service().poll(at(MORNING)).await, Poll::Sent(1));
    }

    #[tokio::test]
    async fn a_data_folder_that_cannot_be_written_does_not_stop_alerts() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(200, 1).await;
        let uri = setup.server.uri();
        let missing = setup.dir.path().join("missing");
        let mut service = Service::new(
            Config::parse(EXAMPLE).unwrap(),
            Prim::new(&uri, "key"),
            Sender::new(account(&format!("{uri}/token")), &uri),
            vec!["phone".into()],
            &missing,
        );
        assert_eq!(service.poll(at(MORNING)).await, Poll::Sent(1));
        assert!(!missing.exists());
    }

    /// One route for line L through the example's stops; none for line X.
    async fn routes_server(status: u16, times: u64) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(path("/v2/navitia/lines/line%3AIDFM%3AC01740/routes"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(serde_json::json!({ "routes": [{ "id": "route:IDFM:A" }] })),
            )
            .expect(times)
            .mount(&server)
            .await;
        let rows: Vec<_> = ["71100", "71250", "71370"]
            .iter()
            .map(|n| serde_json::json!({ "stop_point": { "id": format!("sp:{n}"), "name": n, "stop_area": { "id": format!("stop_area:IDFM:{n}") } } }))
            .collect();
        Mock::given(path("/v2/navitia/routes/route%3AIDFM%3AA/route_schedules"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "route_schedules": [{ "table": { "rows": rows } }] }),
            ))
            .mount(&server)
            .await;
        Mock::given(path("/v2/navitia/lines/line%3AIDFM%3AC02000/routes"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "routes": [] })),
            )
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn routes_are_read_once_a_day_and_saved() {
        let setup = Setup::with(routes_server(200, 1).await).await;
        setup.prim_answers(200, false, 2).await;
        let mut service = setup.service();
        assert_eq!(service.poll(at(MORNING)).await, Poll::Sent(0));
        assert_eq!(service.poll(at(TWO_MINUTES_LATER)).await, Poll::Sent(0));
        let saved = std::fs::read_to_string(setup.dir.path().join("routes.json")).unwrap();
        assert!(saved.contains("stop_area:IDFM:71250"), "{saved}");
    }

    #[tokio::test]
    async fn routes_that_cannot_be_read_leave_the_saved_ones() {
        let setup = Setup::with(routes_server(500, 1).await).await;
        setup.prim_answers(200, false, 1).await;
        let saved = r#"{"C01740":[{"stops":[{"stop_point":"sp:1","stop_area":"stop_area:IDFM:71100","name":"1"}]}]}"#;
        std::fs::write(setup.dir.path().join("routes.json"), saved).unwrap();
        assert_eq!(setup.service().poll(at(MORNING)).await, Poll::Sent(0));
        let kept = std::fs::read_to_string(setup.dir.path().join("routes.json")).unwrap();
        assert!(kept.contains("stop_area:IDFM:71100"), "{kept}");
    }
}
