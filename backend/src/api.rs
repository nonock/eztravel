//! The API the app calls: register the phone, read and set the time away and
//! the usual train of each way, see what the backend sees, the next
//! departures, and pick one of them for the reminder to leave.
//!
//! Every request carries the app key (`Authorization: Bearer …`), compiled
//! into the user's own build of the app and kept out of the repository.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use aws_lc_rs::constant_time::verify_slices_are_equal;
use axum::extract::{FromRef, Request, State};
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use jiff::Timestamp;
use jiff::civil::{Date, Time};
use serde::{Deserialize, Serialize};

use crate::calendar::{Calendar, Direction, Schedule};
use crate::departures::{self, NextDepartures};
use crate::history::History;
use crate::journal::Journal;
use crate::leaving::Refusal;
use crate::line_alerts::{self, LineAlerts};
use crate::profile::{Store, UsualTrains};
use crate::watch::{Snapshot, View};

/// Firebase tokens are a few hundred characters long.
const MAX_TOKEN_LENGTH: usize = 4096;
/// Journeys are named in a few dozen characters.
const MAX_JOURNEY_LENGTH: usize = 256;

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Last day away from the office, included. Required, but may be null.
    #[serde(deserialize_with = "Option::deserialize")]
    pub away_through: Option<Date>,
}

/// `GET /settings`, in the shape fixed by `fixtures/api/settings.json`.
#[derive(Serialize)]
struct SettingsView {
    away_through: Option<Date>,
    usual_trains: UsualTrains,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Way {
    Outbound,
    Inbound,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Usual {
    direction: Way,
    /// The usual train's time at its station. Required, but null clears it.
    #[serde(deserialize_with = "Option::deserialize")]
    time: Option<Time>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Device {
    token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Picked {
    /// The train, by its journey. Required, but null drops the pick.
    #[serde(deserialize_with = "Option::deserialize")]
    journey: Option<String>,
}

/// Why a pick is refused, in the shape fixed by `fixtures/api/pick-refused.json`.
#[derive(Serialize)]
struct Refused {
    refused: Refusal,
}

/// What the handlers share with the poll loop.
#[derive(Clone)]
struct Shared {
    store: Arc<Store>,
    status: Arc<Mutex<Snapshot>>,
    schedule: Arc<Schedule>,
    history: Arc<History>,
    departures: Arc<NextDepartures>,
    line_alerts: Arc<LineAlerts>,
    journal: Arc<Journal>,
}

impl FromRef<Shared> for Arc<Journal> {
    fn from_ref(shared: &Shared) -> Self {
        shared.journal.clone()
    }
}

impl FromRef<Shared> for Arc<LineAlerts> {
    fn from_ref(shared: &Shared) -> Self {
        shared.line_alerts.clone()
    }
}

impl FromRef<Shared> for Arc<Store> {
    fn from_ref(shared: &Shared) -> Self {
        shared.store.clone()
    }
}

impl FromRef<Shared> for Arc<Mutex<Snapshot>> {
    fn from_ref(shared: &Shared) -> Self {
        shared.status.clone()
    }
}

impl FromRef<Shared> for Arc<History> {
    fn from_ref(shared: &Shared) -> Self {
        shared.history.clone()
    }
}

impl FromRef<Shared> for Arc<Schedule> {
    fn from_ref(shared: &Shared) -> Self {
        shared.schedule.clone()
    }
}

impl FromRef<Shared> for Arc<NextDepartures> {
    fn from_ref(shared: &Shared) -> Self {
        shared.departures.clone()
    }
}

/// What the API reads and changes, shared with the poll loop.
pub struct Sources {
    pub store: Arc<Store>,
    pub status: Arc<Mutex<Snapshot>>,
    pub schedule: Schedule,
    pub history: Arc<History>,
    pub departures: Arc<NextDepartures>,
    pub line_alerts: Arc<LineAlerts>,
    pub journal: Arc<Journal>,
}

pub fn router(sources: Sources, key: String) -> Router {
    let Sources {
        store,
        status,
        schedule,
        history,
        departures,
        line_alerts,
        journal,
    } = sources;
    Router::new()
        .route("/devices", post(register))
        .route("/settings", get(settings).put(set_settings))
        .route("/status", get(status_now))
        .route("/history", get(sent))
        .route("/departures", get(next_departures))
        .route("/departure", put(pick_departure))
        .route("/usual-train", put(set_usual_train))
        .route("/alerts", get(alerts))
        .route("/trains", get(ended_trains))
        .layer(middleware::from_fn_with_state(Arc::new(key), authorize))
        .with_state(Shared {
            store,
            status,
            schedule: Arc::new(schedule),
            history,
            departures,
            line_alerts,
            journal,
        })
}

async fn authorize(State(key): State<Arc<String>>, request: Request, next: Next) -> Response {
    let given = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    // Compared in constant time, so that timing does not reveal the key.
    match given {
        Some(given) if verify_slices_are_equal(given.as_bytes(), key.as_bytes()).is_ok() => {
            next.run(request).await
        }
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn register(State(store): State<Arc<Store>>, Json(device): Json<Device>) -> StatusCode {
    if device.token.is_empty() || device.token.len() > MAX_TOKEN_LENGTH {
        return StatusCode::UNPROCESSABLE_ENTITY;
    }
    store.update(|profile| profile.register(&device.token));
    StatusCode::NO_CONTENT
}

async fn settings(State(store): State<Arc<Store>>) -> Json<SettingsView> {
    let profile = store.profile();
    Json(SettingsView {
        away_through: profile.away_through,
        usual_trains: profile.usual,
    })
}

/// As of the last poll; the outage is judged now, so that a stopped poll loop shows.
async fn status_now(
    State(status): State<Arc<Mutex<Snapshot>>>,
    State(store): State<Arc<Store>>,
    State(schedule): State<Arc<Schedule>>,
) -> Json<View> {
    let now = Timestamp::now();
    // From the time away as it is now, not as of the last poll: the app reads this right after a change.
    let next = schedule.next_window(now, store.profile().away_through);
    Json(
        status
            .lock()
            .expect("no thread panics while holding the status")
            .view(now, next),
    )
}

/// How many entries `GET /history` gives, newest first.
const HISTORY_SHOWN: usize = 200;

/// `GET /history`, in the shape fixed by `fixtures/api/history.json`.
#[derive(Serialize)]
struct Sent {
    entries: Vec<SentEntry>,
}

#[derive(Serialize)]
struct SentEntry {
    at: Timestamp,
    data: BTreeMap<String, String>,
}

async fn sent(State(history): State<Arc<History>>) -> Json<Sent> {
    let entries = history
        .latest(HISTORY_SHOWN)
        .into_iter()
        .map(|entry| SentEntry {
            at: entry.at,
            data: entry.data,
        })
        .collect();
    Json(Sent { entries })
}

/// How many trains `GET /trains` gives, the latest first: a few weeks.
const TRAINS_SHOWN: usize = 1000;

/// `GET /trains`, in the shape fixed by `fixtures/api/trains.json`.
#[derive(Serialize)]
struct EndedTrains {
    trains: Vec<crate::trains::Ended>,
}

async fn ended_trains(State(journal): State<Arc<Journal>>) -> Json<EndedTrains> {
    Json(EndedTrains {
        trains: journal.latest(TRAINS_SHOWN),
    })
}

/// Read from PRIM when asked, at most once a minute per stop.
async fn next_departures(
    State(departures): State<Arc<NextDepartures>>,
    State(store): State<Arc<Store>>,
) -> Json<departures::View> {
    let profile = store.profile();
    Json(
        departures
            .view(Timestamp::now(), profile.pick.as_ref(), &profile.usual)
            .await,
    )
}

/// The lines' disruptions, as the poll loop last read them, or read at most every few minutes.
async fn alerts(State(line_alerts): State<Arc<LineAlerts>>) -> Json<line_alerts::View> {
    Json(line_alerts.view(Timestamp::now()).await)
}

async fn pick_departure(
    State(store): State<Arc<Store>>,
    State(departures): State<Arc<NextDepartures>>,
    State(schedule): State<Arc<Schedule>>,
    Json(picked): Json<Picked>,
) -> Response {
    pick(
        &store,
        &departures,
        &schedule,
        picked.journey,
        Timestamp::now(),
    )
    .await
}

/// Keeps the train `journey` for the reminder to leave, if it can be reminded
/// of; none drops the pick.
async fn pick(
    store: &Store,
    departures: &NextDepartures,
    schedule: &Schedule,
    journey: Option<String>,
    now: Timestamp,
) -> Response {
    let Some(journey) = journey else {
        store.update(|profile| profile.pick = None);
        return StatusCode::NO_CONTENT.into_response();
    };
    let calendar = Calendar {
        away_through: store.profile().away_through,
        ..schedule.calendar.clone()
    };
    let picked = if journey.is_empty() || journey.len() > MAX_JOURNEY_LENGTH {
        Err(Refusal::Unknown)
    } else {
        departures.pick(&journey, now, &calendar).await
    };
    match picked {
        Ok(pick) => {
            store.update(|profile| profile.pick = Some(pick));
            StatusCode::NO_CONTENT.into_response()
        }
        Err(refused) => {
            (StatusCode::UNPROCESSABLE_ENTITY, Json(Refused { refused })).into_response()
        }
    }
}

/// Keeps the usual train of a way, if it can be reminded of; none clears it.
async fn set_usual_train(
    State(store): State<Arc<Store>>,
    State(departures): State<Arc<NextDepartures>>,
    Json(usual): Json<Usual>,
) -> Response {
    let direction = match usual.direction {
        Way::Outbound => Direction::Outbound,
        Way::Inbound => Direction::Inbound,
    };
    if let Some(Err(refused)) = usual
        .time
        .map(|time| departures.check_usual(direction, time))
    {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(Refused { refused })).into_response();
    }
    store.update(|profile| profile.usual.set(direction, usual.time));
    StatusCode::NO_CONTENT.into_response()
}

async fn set_settings(
    State(store): State<Arc<Store>>,
    Json(settings): Json<Settings>,
) -> StatusCode {
    store.update(|profile| profile.away_through = settings.away_through);
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::Direction;
    use crate::config::Config;
    use crate::history::History;
    use crate::prim::Prim;
    use crate::profile::Pick;
    use crate::trains::Ended;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use http_body_util::BodyExt;
    use jiff::civil::date;
    use jiff::{SignedDuration, Timestamp};
    use tower::ServiceExt;

    const KEY: &str = "the-app-key";

    struct Api {
        store: Arc<Store>,
        status: Arc<Mutex<Snapshot>>,
        history: Arc<History>,
        departures: Arc<NextDepartures>,
        line_alerts: Arc<LineAlerts>,
        journal: Arc<Journal>,
        _dir: tempfile::TempDir,
    }

    impl Api {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(Store::open(&dir.path().join("profile.json"), &[]));
            // No routes yet: the stops of the legs are unknown, and PRIM is not called.
            let departures = NextDepartures::new(
                &Config::parse(include_str!("../config.example.toml")).unwrap(),
                Prim::new("http://127.0.0.1:9", "key"),
                Default::default(),
            );
            Api {
                store,
                status: Arc::default(),
                history: Arc::new(History::open(&dir.path().join("history.json"))),
                departures: Arc::new(departures),
                line_alerts: Arc::new(LineAlerts::new(
                    &Config::parse(include_str!("../config.example.toml")).unwrap(),
                    Prim::new("http://127.0.0.1:9", "key"),
                )),
                journal: Arc::new(Journal::open(&dir.path().join("journal.jsonl"))),
                _dir: dir,
            }
        }

        async fn call(
            &self,
            method: Method,
            uri: &str,
            authorization: Option<&str>,
            body: Option<&str>,
        ) -> (StatusCode, String) {
            let mut request = Request::builder().method(method).uri(uri);
            if let Some(authorization) = authorization {
                request = request.header("authorization", authorization);
            }
            if body.is_some() {
                request = request.header("content-type", "application/json");
            }
            let request = request
                .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                .unwrap();
            let schedule = Config::parse(include_str!("../config.example.toml"))
                .unwrap()
                .schedule();
            let sources = Sources {
                store: self.store.clone(),
                status: self.status.clone(),
                schedule,
                history: self.history.clone(),
                departures: self.departures.clone(),
                line_alerts: self.line_alerts.clone(),
                journal: self.journal.clone(),
            };
            let response = router(sources, KEY.into()).oneshot(request).await.unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            (status, String::from_utf8(bytes.to_vec()).unwrap())
        }

        async fn authorized(
            &self,
            method: Method,
            uri: &str,
            body: Option<&str>,
        ) -> (StatusCode, String) {
            self.call(method, uri, Some(&format!("Bearer {KEY}")), body)
                .await
        }
    }

    #[tokio::test]
    async fn every_route_wants_the_app_key() {
        let api = Api::new();
        for (method, uri, body) in [
            (Method::POST, "/devices", Some(r#"{"token":"t"}"#)),
            (Method::GET, "/settings", None),
            (Method::PUT, "/settings", Some(r#"{"away_through":null}"#)),
            (Method::GET, "/status", None),
            (Method::GET, "/history", None),
            (Method::GET, "/departures", None),
            (Method::PUT, "/departure", Some(r#"{"journey":null}"#)),
            (Method::GET, "/alerts", None),
            (Method::GET, "/trains", None),
            (
                Method::PUT,
                "/usual-train",
                Some(r#"{"direction":"outbound","time":null}"#),
            ),
        ] {
            for authorization in [
                None,
                Some("Bearer wrong-key"),
                Some("Bearer the-app-ke"),
                Some(KEY),
                Some("Basic the-app-key"),
            ] {
                let (status, _) = api.call(method.clone(), uri, authorization, body).await;
                assert_eq!(
                    status,
                    StatusCode::UNAUTHORIZED,
                    "{method} {uri} with {authorization:?}"
                );
            }
        }
        assert_eq!(api.store.profile().devices, Vec::<String>::new());
    }

    #[tokio::test]
    async fn the_phone_registers_its_token() {
        let api = Api::new();
        assert_eq!(
            api.authorized(Method::POST, "/devices", Some(r#"{"token":"phone-1"}"#))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            api.authorized(Method::POST, "/devices", Some(r#"{"token":"phone-2"}"#))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(api.store.profile().devices, ["phone-1", "phone-2"]);
    }

    #[tokio::test]
    async fn a_malformed_registration_is_refused() {
        let api = Api::new();
        assert_eq!(
            api.authorized(Method::POST, "/devices", Some(r#"{"token":""}"#))
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let long = format!(r#"{{"token":"{}"}}"#, "x".repeat(5000));
        assert_eq!(
            api.authorized(Method::POST, "/devices", Some(&long))
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            api.authorized(Method::POST, "/devices", Some(r#"{"token":"#))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            api.authorized(Method::POST, "/devices", Some(r#"{"name":"x"}"#))
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            api.authorized(Method::GET, "/devices", None).await.0,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(api.store.profile().devices, Vec::<String>::new());
    }

    #[tokio::test]
    async fn the_time_away_is_read_set_and_cleared() {
        let api = Api::new();
        assert_eq!(
            api.authorized(Method::GET, "/settings", None).await,
            (
                StatusCode::OK,
                r#"{"away_through":null,"usual_trains":{"outbound":null,"inbound":null}}"#.into()
            )
        );
        let set = api
            .authorized(
                Method::PUT,
                "/settings",
                Some(r#"{"away_through":"2026-12-24"}"#),
            )
            .await;
        assert_eq!(set.0, StatusCode::NO_CONTENT);
        assert_eq!(api.store.profile().away_through, Some(date(2026, 12, 24)));
        let (_, body) = api.authorized(Method::GET, "/settings", None).await;
        assert!(
            body.starts_with(r#"{"away_through":"2026-12-24","#),
            "{body}"
        );
        api.authorized(Method::PUT, "/settings", Some(r#"{"away_through":null}"#))
            .await;
        assert_eq!(api.store.profile().away_through, None);
    }

    #[tokio::test]
    async fn the_usual_train_of_each_way_is_set_read_and_cleared() {
        let api = Api::new();
        let set = |body: &'static str| api.authorized(Method::PUT, "/usual-train", Some(body));
        assert_eq!(
            set(r#"{"direction":"outbound","time":"08:07"}"#).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            set(r#"{"direction":"inbound","time":"17:52:00"}"#).await.0,
            StatusCode::NO_CONTENT
        );
        let path = format!(
            "{}/../fixtures/api/settings.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        api.authorized(
            Method::PUT,
            "/settings",
            Some(r#"{"away_through":"2026-12-24"}"#),
        )
        .await;
        let (_, body) = api.authorized(Method::GET, "/settings", None).await;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            fixture
        );
        // Leaving at 8:18, after the morning window: it would never be reminded.
        assert_eq!(
            set(r#"{"direction":"outbound","time":"08:30"}"#).await,
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                r#"{"refused":"outside_window"}"#.into()
            )
        );
        assert_eq!(
            set(r#"{"direction":"inbound","time":null}"#).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            api.store.profile().usual,
            crate::profile::UsualTrains {
                outbound: Some(jiff::civil::time(8, 7, 0, 0)),
                inbound: None
            }
        );
        for body in [
            r#"{"direction":"sideways","time":null}"#,
            r#"{"direction":"outbound","time":"25:00"}"#,
            r#"{"direction":"outbound"}"#,
            r#"{"time":"08:07"}"#,
            r#"{"direction":"outbound","time":null,"other":1}"#,
        ] {
            assert_eq!(
                set(body).await.0,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{body}"
            );
        }
    }

    #[tokio::test]
    async fn the_trains_kept_answer_as_in_the_fixture() {
        let api = Api::new();
        let path = format!("{}/../fixtures/api/trains.json", env!("CARGO_MANIFEST_DIR"));
        let fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        // Written as they ended, the oldest first.
        let mut kept: Vec<Ended> = serde_json::from_value(fixture["trains"].clone()).unwrap();
        kept.reverse();
        api.journal.append(&kept);
        let (status, body) = api.authorized(Method::GET, "/trains", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            fixture
        );
    }

    #[tokio::test]
    async fn the_status_is_what_the_poll_loop_last_saw() {
        let api = Api::new();
        // Nothing polled yet: nothing to say but when the next window opens.
        let (status, body) = api.authorized(Method::GET, "/status", None).await;
        assert_eq!(status, StatusCode::OK);
        let next = body
            .split(r#""next":""#)
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        assert_eq!(
            body,
            format!(
                r#"{{"watching":null,"checked_at":null,"next":"{next}","outage":null,"lines":[],"planned":[]}}"#
            )
        );
        // A poll loop silent for an hour has stopped.
        let an_hour_ago = Timestamp::now() - SignedDuration::from_hours(1);
        api.status.lock().unwrap().polled_at = Some(an_hour_ago);
        let (_, body) = api.authorized(Method::GET, "/status", None).await;
        let status: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(status["outage"]["cause"], "server");
        assert_eq!(status["outage"]["since"], an_hour_ago.to_string());
    }

    /// When the next window opens, as `GET /status` says, in Paris.
    async fn next_watch(api: &Api) -> jiff::Zoned {
        let (_, body) = api.authorized(Method::GET, "/status", None).await;
        let status: serde_json::Value = serde_json::from_str(&body).unwrap();
        let next: Timestamp = status["next"].as_str().unwrap().parse().unwrap();
        next.in_tz("Europe/Paris").unwrap()
    }

    #[tokio::test]
    async fn the_next_watch_follows_the_time_away_at_once() {
        // The app reads the status right after saving: it must already show the change.
        let api = Api::new();
        let before = next_watch(&api).await;
        let away = format!(r#"{{"away_through":"{}"}}"#, before.date());
        api.authorized(Method::PUT, "/settings", Some(&away)).await;
        let after = next_watch(&api).await;
        assert!(after.date() > before.date(), "{before} then {after}");
    }

    #[tokio::test]
    async fn the_history_answers_as_in_the_fixture() {
        let api = Api::new();
        let path = format!(
            "{}/../fixtures/api/history.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        // Recorded oldest first, as the poll loop would.
        for entry in fixture["entries"].as_array().unwrap().iter().rev() {
            api.history.record(crate::history::Entry {
                at: entry["at"].as_str().unwrap().parse().unwrap(),
                data: serde_json::from_value(entry["data"].clone()).unwrap(),
                delivered: 1,
                records: vec![],
            });
        }
        let (status, body) = api.authorized(Method::GET, "/history", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            fixture
        );
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!("{}/../fixtures/api/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn a_pick_is_dropped_with_null() {
        let api = Api::new();
        api.store.update(|profile| {
            profile.pick = Some(Pick {
                day: date(2026, 11, 3),
                direction: Direction::Inbound,
                journey: "j1".into(),
                leave: "2026-11-03T15:20:00Z".parse().unwrap(),
            });
        });
        let (status, _) = api
            .authorized(Method::PUT, "/departure", Some(r#"{"journey":null}"#))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(api.store.profile().pick, None);
    }

    #[tokio::test]
    async fn a_pick_says_why_it_is_refused() {
        // Without the routes, the departures are unknown.
        let api = Api::new();
        let (status, body) = api
            .authorized(Method::PUT, "/departure", Some(r#"{"journey":"j2"}"#))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            fixture("pick-refused")
        );
        let long = format!(r#"{{"journey":"{}"}}"#, "j".repeat(MAX_JOURNEY_LENGTH + 1));
        for request in [r#"{"journey":""}"#, long.as_str()] {
            let (status, body) = api
                .authorized(Method::PUT, "/departure", Some(request))
                .await;
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(body, r#"{"refused":"unknown"}"#);
        }
        for request in [r#"{"journey":"j2","at":"now"}"#, "{}", r#"{"journey":"#] {
            let (status, _) = api
                .authorized(Method::PUT, "/departure", Some(request))
                .await;
            assert!(status.is_client_error(), "{request}: {status}");
        }
        assert_eq!(api.store.profile().pick, None);
    }

    #[tokio::test]
    async fn a_train_picked_is_kept_and_shown() {
        let server = wiremock::MockServer::start().await;
        for (stop, body) in [
            (
                "STIF:StopArea:SP:100:",
                departures::example::morning_board(),
            ),
            (
                "STIF:StopArea:SP:200:",
                departures::example::morning_alight(),
            ),
        ] {
            wiremock::Mock::given(wiremock::matchers::query_param("MonitoringRef", stop))
                .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }
        let config = Config::parse(include_str!("../config.example.toml")).unwrap();
        let departures = NextDepartures::new(
            &config,
            Prim::new(&server.uri(), "key"),
            departures::example::routes(),
        );
        let api = Api::new();
        // 7:50 in Paris: the 8:07, 4 minutes late, leaves at 8:11; leave at 7:59.
        let now = departures::example::at("06:50:00");
        let response = pick(
            &api.store,
            &departures,
            &config.schedule(),
            Some("j2".into()),
            now,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let picked = api.store.profile().pick.unwrap();
        assert_eq!(
            (picked.direction, picked.journey.as_str(), picked.leave),
            (
                Direction::Outbound,
                "j2",
                departures::example::at("06:59:00")
            )
        );
        // Away that day: refused, and the pick stays as it was.
        api.store
            .update(|profile| profile.away_through = Some(date(2026, 11, 3)));
        let response = pick(
            &api.store,
            &departures,
            &config.schedule(),
            Some("j2".into()),
            now,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(api.store.profile().pick, Some(picked));
    }

    #[tokio::test]
    async fn alerts_that_cannot_be_read_are_said_unknown() {
        let api = Api::new();
        let (status, body) = api.authorized(Method::GET, "/alerts", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"lines":null}"#);
    }

    #[tokio::test]
    async fn the_departures_are_read_when_asked() {
        let api = Api::new();
        let (status, body) = api.authorized(Method::GET, "/departures", None).await;
        assert_eq!(status, StatusCode::OK);
        let view: serde_json::Value = serde_json::from_str(&body).unwrap();
        let legs = view["legs"].as_array().unwrap();
        assert_eq!(legs.len(), 2);
        assert!(legs.iter().all(|leg| leg["departures"].is_null()), "{body}");
    }

    #[tokio::test]
    async fn malformed_settings_are_refused() {
        let api = Api::new();
        for body in [
            r#"{"away_through":"2026-13-40"}"#,
            r#"{"away_through":null,"other":1}"#,
            r#"{}"#,
        ] {
            assert_eq!(
                api.authorized(Method::PUT, "/settings", Some(body)).await.0,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{body}"
            );
        }
        assert_eq!(
            api.authorized(Method::DELETE, "/settings", None).await.0,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            api.authorized(Method::GET, "/elsewhere", None).await.0,
            StatusCode::NOT_FOUND
        );
    }
}
