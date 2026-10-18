//! One poll of the service: what to fetch, what to send, what to remember.
//!
//! The alert state is committed only once every notification it decided has
//! been sent, so a failed send is retried at the next poll: a rare duplicate
//! is better than a missed alert. A failed read of PRIM decides nothing,
//! since an empty answer would read as "the disruption is over". Failures
//! are counted in `watch`, which tells the phone when they last.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::alerts::{Commute, Notification, State, WatchedLeg, decide};
use crate::api;
use crate::calendar::{Calendar, Direction, watched_direction};
use crate::config::Config;
use crate::prim::Prim;
use crate::profile::{Profile, Store};
use crate::push::{self, PushError, Sender};
use crate::relevance::{Leg, Route};
use crate::watch::{Cause, Health, LineStatus, Snapshot};

const STATE_FILE: &str = "state.json";
const PRIM_URL: &str = "https://prim.iledefrance-mobilites.fr/marketplace";
const FCM_URL: &str = "https://fcm.googleapis.com";
const ROUTES_FILE: &str = "routes.json";
const PROFILE_FILE: &str = "profile.json";
const HEALTH_FILE: &str = "health.json";

pub struct Service {
    config: Config,
    prim: Prim,
    sender: Sender,
    store: Arc<Store>,
    data_dir: PathBuf,
    state: State,
    health: Health,
    status: Arc<Mutex<Snapshot>>,
    routes: BTreeMap<String, Vec<Route>>,
    routes_day: Option<Date>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    /// Not an office day, or outside the watch windows.
    Idle,
    /// No device registered yet: nothing decided, so that a phone registering
    /// during a window still hears about what is under way.
    NoDevice,
    /// PRIM could not be read: nothing decided, nothing sent.
    Unavailable(String),
    /// Every notification decided was sent (possibly none).
    Sent(usize),
    /// A send failed: the state stays as it was, so the next poll tries again.
    NotSent(String),
}

/// What `main` runs: the poll loop, and the API that shares its profile.
pub struct Startup {
    pub service: Service,
    pub api: axum::Router,
}

impl Service {
    /// Picks up the state and routes saved in `data_dir` by a previous run, if any.
    pub fn new(
        config: Config,
        prim: Prim,
        sender: Sender,
        store: Arc<Store>,
        data_dir: &Path,
    ) -> Self {
        Self {
            state: load(&data_dir.join(STATE_FILE)).unwrap_or_default(),
            health: load(&data_dir.join(HEALTH_FILE)).unwrap_or_default(),
            status: Arc::default(),
            routes: load(&data_dir.join(ROUTES_FILE)).unwrap_or_default(),
            routes_day: None,
            config,
            prim,
            sender,
            store,
            data_dir: data_dir.to_path_buf(),
        }
    }

    /// Everything from environment variables, as on fly.io where they are secrets:
    /// `EZTRAVEL_CONFIG` (the configuration file's text), `PRIM_API_KEY`,
    /// `FIREBASE_SERVICE_ACCOUNT` (the key file's text), `APP_KEY` (the key the
    /// app sends to the API) and, optionally, `DEVICE_TOKENS` (devices to start
    /// with, separated by commas, spaces or new lines), `EZTRAVEL_DATA`
    /// (default `/data`), `PRIM_URL` and `FCM_URL`.
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Result<Startup, String> {
        let required = |name: &str| var(name).ok_or_else(|| format!("{name} is not set"));
        let config = Config::parse(&required("EZTRAVEL_CONFIG")?)
            .map_err(|error| format!("EZTRAVEL_CONFIG: {error}"))?;
        let account = serde_json::from_str(&required("FIREBASE_SERVICE_ACCOUNT")?)
            .map_err(|error| format!("FIREBASE_SERVICE_ACCOUNT: {error}"))?;
        let app_key = required("APP_KEY")?;
        if app_key.len() < 32 {
            return Err("APP_KEY is too short: 32 characters at least".into());
        }
        let seed: Vec<String> = var("DEVICE_TOKENS")
            .unwrap_or_default()
            .split([',', ' ', '\n'])
            .filter(|token| !token.is_empty())
            .map(String::from)
            .collect();
        let prim = Prim::new(
            &var("PRIM_URL").unwrap_or(PRIM_URL.into()),
            &required("PRIM_API_KEY")?,
        );
        let sender = Sender::new(account, &var("FCM_URL").unwrap_or(FCM_URL.into()));
        let data_dir = PathBuf::from(var("EZTRAVEL_DATA").unwrap_or("/data".into()));
        let store = Arc::new(Store::open(&data_dir.join(PROFILE_FILE), &seed));
        let schedule = config.schedule();
        let service = Self::new(config, prim, sender, store.clone(), &data_dir);
        Ok(Startup {
            api: api::router(store, service.status(), schedule, app_key),
            service,
        })
    }

    /// What the last poll saw, updated after each poll.
    pub fn status(&self) -> Arc<Mutex<Snapshot>> {
        self.status.clone()
    }

    /// The local time, what is watched and who is notified, to check the service where it runs.
    pub fn describe(&self, now: Timestamp) -> String {
        let profile = self.store.profile();
        let time_zone = self.config.time_zone();
        let direction = watched_direction(
            now,
            &time_zone,
            &self.calendar(&profile.away_through),
            &self.config.windows,
        );
        let away = profile.away_through.map_or("not away".to_string(), |last| {
            format!("away through {last}")
        });
        format!(
            "local time {}, watching {direction:?}, {away}, {} device(s)",
            now.to_zoned(time_zone).strftime("%A %Y-%m-%d %H:%M %Z"),
            profile.devices.len()
        )
    }

    /// The configured calendar, with the time away set from the app.
    fn calendar(&self, away_through: &Option<jiff::civil::Date>) -> Calendar {
        Calendar {
            away_through: *away_through,
            ..self.config.calendar.clone()
        }
    }

    pub async fn poll(&mut self, now: Timestamp) -> Poll {
        // A copy: no lock is held while PRIM and Firebase are called.
        let profile = self.store.profile();
        let time_zone = self.config.time_zone();
        let local = now.to_zoned(time_zone.clone()).datetime();
        let calendar = self.calendar(&profile.away_through);
        let direction = watched_direction(now, &time_zone, &calendar, &self.config.windows);
        let mut health = self.health.clone();
        let outcome = match direction {
            None => {
                let mut next = self.state.clone();
                decide(&mut next, local, None, &self.commute(), &[]);
                self.commit(next);
                health.idle();
                Poll::Idle
            }
            Some(direction) => {
                health.watching(now, local.date(), direction);
                self.watch(now, local, direction, &profile, &mut health)
                    .await
            }
        };
        if health != self.health {
            self.health = health;
            save(&self.data_dir.join(HEALTH_FILE), &self.health);
        }
        self.publish(now, local.date(), direction);
        outcome
    }

    /// A poll during a window.
    async fn watch(
        &mut self,
        now: Timestamp,
        local: DateTime,
        direction: Direction,
        profile: &Profile,
        health: &mut Health,
    ) -> Poll {
        if profile.devices.is_empty() {
            return Poll::NoDevice;
        }
        self.refresh_routes(local.date()).await;
        let lines = self.config.line_codes();
        let codes: Vec<&str> = lines.iter().map(String::as_str).collect();
        let disruptions = match self.prim.disruptions(&codes).await {
            Ok(disruptions) => disruptions,
            Err(error) => {
                self.failed(now, Cause::Prim, profile, health).await;
                return Poll::Unavailable(format!("{error:?}"));
            }
        };
        let mut next = self.state.clone();
        let mut notifications = decide(
            &mut next,
            local,
            Some(direction),
            &self.commute(),
            &disruptions,
        );
        if health.recovering() {
            notifications.insert(0, Notification::Recovered);
        }
        if let Err(error) = self.send(now, profile, &notifications).await {
            self.failed(now, Cause::Firebase, profile, health).await;
            return Poll::NotSent(format!("{error:?}"));
        }
        self.commit(next);
        health.checked(now);
        Poll::Sent(notifications.len())
    }

    /// Tells the phone when polls have failed for too long.
    async fn failed(
        &mut self,
        now: Timestamp,
        cause: Cause,
        profile: &Profile,
        health: &mut Health,
    ) {
        if health.failed(now, cause)
            && self
                .send(now, profile, &[Notification::Outage])
                .await
                .is_ok()
        {
            health.outage_sent();
        }
    }

    /// Every notification to every device, or the first error.
    async fn send(
        &mut self,
        now: Timestamp,
        profile: &Profile,
        notifications: &[Notification],
    ) -> Result<(), PushError> {
        for notification in notifications {
            let data = push::data(notification, &self.config.lines);
            for device in &profile.devices {
                match self.sender.send(now, device, &data).await {
                    Ok(()) => {}
                    Err(PushError::Unregistered) => {
                        eprintln!(
                            "Firebase no longer knows a device: forgotten until the app registers again"
                        );
                        self.store.update(|profile| profile.forget(device));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }

    /// What the app's home screen shows: the lines of the way being watched, and how polls go.
    fn publish(&self, now: Timestamp, today: Date, direction: Option<Direction>) {
        let legs: &[Leg] = match direction {
            Some(Direction::Outbound) => &self.config.outbound,
            Some(Direction::Inbound) => &self.config.inbound,
            None => &[],
        };
        let lines = legs
            .iter()
            .map(|leg| {
                let (line, mode) = push::label(&leg.line, &self.config.lines);
                LineStatus {
                    line,
                    mode,
                    severity: self.state.severity(&leg.line).map(push::severity_name),
                }
            })
            .collect();
        let snapshot = Snapshot {
            polled_at: Some(now),
            watching: direction,
            health: self.health.clone(),
            lines,
            planned: self.state.planned(today).to_vec(),
        };
        *self
            .status
            .lock()
            .expect("no thread panics while holding the status") = snapshot;
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

pub(crate) fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
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
pub(crate) fn save<T: Serialize>(path: &Path, value: &T) {
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
    use crate::watch::{Cause, LineStatus, Outage};
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
        store: Arc<Store>,
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
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(Store::open(
                &dir.path().join("profile.json"),
                &["phone".into()],
            ));
            Setup { server, dir, store }
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
            self.firebase_answers_to("started", status, times).await;
        }

        /// Answers notifications of `kind` (their `type`).
        async fn firebase_answers_to(&self, kind: &str, status: u16, times: u64) {
            let body = if status == 404 { "UNREGISTERED" } else { "" };
            let mock = Mock::given(method("POST"))
                .and(path("/v1/projects/demo-project/messages:send"))
                .and(body_string_contains(format!(r#""type":"{kind}""#)))
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
                self.store.clone(),
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
        assert_eq!(setup.store.profile().devices, Vec::<String>::new());
    }

    #[tokio::test]
    async fn without_a_device_nothing_is_decided() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 0).await;
        setup.store.update(|profile| profile.forget("phone"));
        assert_eq!(setup.service().poll(at(MORNING)).await, Poll::NoDevice);
        assert_eq!(setup.saved_state(), None);
    }

    #[tokio::test]
    async fn nothing_is_watched_while_away() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 0).await;
        setup
            .store
            .update(|profile| profile.away_through = Some(jiff::civil::date(2026, 11, 3)));
        assert_eq!(setup.service().poll(at(MORNING)).await, Poll::Idle);
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
            setup.store.clone(),
            &missing,
        );
        assert_eq!(service.poll(at(MORNING)).await, Poll::Sent(1));
        assert!(!missing.exists());
    }

    const APP_KEY: &str = "an-app-key-of-at-least-32-characters";

    fn environment(overrides: &[(&str, Option<&str>)]) -> impl Fn(&str) -> Option<String> {
        let account = serde_json::json!({
            "type": "service_account", "project_id": "demo-project",
            "client_email": "sender@demo-project.iam.gserviceaccount.com",
            "private_key": crate::push::testing::test_key().0, "token_uri": "https://oauth2.example/token"
        });
        let mut vars: BTreeMap<String, String> = BTreeMap::from([
            ("EZTRAVEL_CONFIG".into(), EXAMPLE.into()),
            ("PRIM_API_KEY".into(), "key".into()),
            ("FIREBASE_SERVICE_ACCOUNT".into(), account.to_string()),
            ("APP_KEY".into(), APP_KEY.into()),
            ("DEVICE_TOKENS".into(), "first,second\nthird  ".into()),
        ]);
        for (name, value) in overrides {
            match value {
                Some(value) => vars.insert(name.to_string(), value.to_string()),
                None => vars.remove(*name),
            };
        }
        move |name: &str| vars.get(name).cloned()
    }

    fn data_folder(dir: &tempfile::TempDir) -> String {
        dir.path().to_str().unwrap().to_string()
    }

    #[test]
    fn starts_from_environment_variables() {
        let dir = tempfile::tempdir().unwrap();
        let startup =
            Service::from_env(environment(&[("EZTRAVEL_DATA", Some(&data_folder(&dir)))])).unwrap();
        assert_eq!(
            startup.service.store.profile().devices,
            ["first", "second", "third"]
        );
        assert_eq!(startup.service.data_dir, dir.path());
        let default = Service::from_env(environment(&[])).unwrap();
        assert_eq!(default.service.data_dir, Path::new("/data"));
        // Devices may also come only from the app.
        let without = Service::from_env(environment(&[
            ("DEVICE_TOKENS", None),
            ("EZTRAVEL_DATA", Some(&data_folder(&dir))),
        ]))
        .unwrap();
        assert_eq!(
            without.service.store.profile().devices,
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn the_api_shares_the_service_profile() {
        let dir = tempfile::tempdir().unwrap();
        let startup =
            Service::from_env(environment(&[("EZTRAVEL_DATA", Some(&data_folder(&dir)))])).unwrap();
        let request = axum::http::Request::builder()
            .method("PUT")
            .uri("/settings")
            .header("authorization", format!("Bearer {APP_KEY}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"away_through":"2026-11-03"}"#))
            .unwrap();
        let response = tower::ServiceExt::oneshot(startup.api, request)
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
        assert!(
            startup
                .service
                .describe(at(MORNING))
                .contains("away through 2026-11-03")
        );
    }

    #[test]
    fn says_which_variable_is_wrong() {
        let error = |overrides: &[(&str, Option<&str>)]| {
            Service::from_env(environment(overrides)).err().unwrap()
        };
        for name in [
            "EZTRAVEL_CONFIG",
            "FIREBASE_SERVICE_ACCOUNT",
            "APP_KEY",
            "PRIM_API_KEY",
        ] {
            assert_eq!(error(&[(name, None)]), format!("{name} is not set"));
        }
        assert!(error(&[("EZTRAVEL_CONFIG", Some("[calendar"))]).contains("EZTRAVEL_CONFIG"));
        assert!(
            error(&[("FIREBASE_SERVICE_ACCOUNT", Some("{}"))]).contains("FIREBASE_SERVICE_ACCOUNT")
        );
        // A short key could be guessed.
        assert!(error(&[("APP_KEY", Some("short"))]).contains("APP_KEY"));
    }

    #[test]
    fn describes_local_time_what_is_watched_and_who_is_notified() {
        let startup = Service::from_env(environment(&[])).unwrap();
        assert_eq!(
            startup.service.describe(at(MORNING)),
            "local time Tuesday 2026-11-03 07:00 CET, watching Some(Outbound), not away, 3 device(s)"
        );
        assert_eq!(
            startup.service.describe(at(LATE_MORNING)),
            "local time Tuesday 2026-11-03 11:00 CET, watching None, not away, 3 device(s)"
        );
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

    /// Tuesday 3 November 2026 at `time` UTC (one hour behind Paris).
    fn tuesday(time: &str) -> Timestamp {
        at(&format!("2026-11-03T{time}:00Z"))
    }

    /// PRIM answers 503 to the first `failures` requests, then a quiet feed.
    async fn prim_fails(setup: &Setup, failures: u64) {
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(failures)
            .mount(&setup.server)
            .await;
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(feed(false)))
            .mount(&setup.server)
            .await;
    }

    #[tokio::test]
    async fn prim_out_for_ten_minutes_is_notified_once_and_so_is_its_end() {
        let setup = Setup::new().await;
        prim_fails(&setup, 3).await;
        setup.firebase_answers_to("outage", 200, 1).await;
        setup.firebase_answers_to("recovered", 200, 1).await;
        let mut service = setup.service();
        let unavailable = Poll::Unavailable("Status(503)".into());
        // The window's first poll is at 7:00 in Paris.
        assert_eq!(service.poll(tuesday("06:00")).await, unavailable);
        assert_eq!(service.poll(tuesday("06:10")).await, unavailable);
        assert_eq!(service.poll(tuesday("06:12")).await, unavailable);
        // A restart does not forget that the outage was notified.
        assert_eq!(setup.service().poll(tuesday("06:14")).await, Poll::Sent(1));
    }

    #[tokio::test]
    async fn an_outage_of_the_morning_is_said_over_in_the_evening() {
        let setup = Setup::new().await;
        prim_fails(&setup, 2).await;
        setup.firebase_answers_to("outage", 200, 1).await;
        setup.firebase_answers_to("recovered", 200, 1).await;
        let mut service = setup.service();
        service.poll(tuesday("06:00")).await;
        service.poll(tuesday("06:10")).await;
        assert_eq!(service.poll(at(LATE_MORNING)).await, Poll::Idle);
        // 16:00 in Paris: PRIM answers again.
        assert_eq!(service.poll(tuesday("15:00")).await, Poll::Sent(1));
    }

    #[tokio::test]
    async fn an_outage_that_could_not_be_sent_is_tried_again() {
        let setup = Setup::new().await;
        prim_fails(&setup, 3).await;
        setup.firebase_answers_to("outage", 500, 1).await;
        setup.firebase_answers_to("outage", 200, 1).await;
        let mut service = setup.service();
        for time in ["06:00", "06:10", "06:12"] {
            service.poll(tuesday(time)).await;
        }
    }

    #[tokio::test]
    async fn notifications_that_cannot_be_sent_are_an_outage_too() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 2).await;
        setup.firebase_answers(500, 2).await;
        // Telling the phone fails as well, most likely.
        setup.firebase_answers_to("outage", 500, 1).await;
        let mut service = setup.service();
        service.poll(tuesday("06:00")).await;
        service.poll(tuesday("06:10")).await;
        let status = service.status();
        let outage = status.lock().unwrap().view(tuesday("06:10"), None).outage;
        assert_eq!(
            outage,
            Some(Outage {
                since: tuesday("06:00"),
                cause: Cause::Firebase
            })
        );
    }

    #[tokio::test]
    async fn each_poll_publishes_what_it_saw() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(200, 1).await;
        let mut service = setup.service();
        let status = service.status();
        service.poll(at(MORNING)).await;
        let line = |line: &str, mode, severity| LineStatus {
            line: line.into(),
            mode,
            severity,
        };
        {
            let seen = status.lock().unwrap();
            let view = seen.view(at(MORNING), None);
            assert_eq!(view.watching, Some("outbound"));
            assert_eq!(view.checked_at, Some(at(MORNING)));
            assert_eq!(
                view.lines,
                [
                    line("L", "train", Some("blocking")),
                    line("X", "metro", None)
                ]
            );
        }
        service.poll(at(LATE_MORNING)).await;
        let seen = status.lock().unwrap();
        let view = seen.view(at(LATE_MORNING), None);
        assert_eq!(
            (view.watching, view.lines, view.checked_at),
            (None, vec![], Some(at(MORNING)))
        );
        assert_eq!(seen.polled_at, Some(at(LATE_MORNING)));
    }

    #[tokio::test]
    async fn the_evening_status_lists_the_lines_of_the_way_home() {
        let setup = Setup::new().await;
        setup.prim_answers(200, false, 1).await;
        let mut service = setup.service();
        // 16:00 in Paris.
        assert_eq!(service.poll(tuesday("15:00")).await, Poll::Sent(0));
        let view = service
            .status()
            .lock()
            .unwrap()
            .view(tuesday("15:00"), None);
        assert_eq!(view.watching, Some("inbound"));
        let names: Vec<&str> = view.lines.iter().map(|line| line.line.as_str()).collect();
        assert_eq!(names, ["X", "L"]);
    }

    #[tokio::test]
    async fn todays_planned_disruptions_are_published() {
        let setup = Setup::new().await;
        let mut feed = feed(true);
        feed["disruptions"][0]["cause"] = "TRAVAUX".into();
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(feed))
            .mount(&setup.server)
            .await;
        setup.firebase_answers_to("planned", 200, 1).await;
        let mut service = setup.service();
        assert_eq!(service.poll(at(MORNING)).await, Poll::Sent(1));
        let status = service.status();
        assert_eq!(
            status.lock().unwrap().planned,
            ["Ligne L : trafic interrompu"]
        );
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
