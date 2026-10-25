//! The API the app calls: register the phone, read and set the days of the
//! calendar, with the school holidays, and the usual train of each way, see
//! what the backend sees, the next departures, and pick one of them for the
//! reminder to leave.
//!
//! Every request carries the app key (`Authorization: Bearer …`), compiled
//! into the user's own build of the app and kept out of the repository.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use aws_lc_rs::constant_time::verify_slices_are_equal;
use axum::extract::{FromRef, Query, Request, State};
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use jiff::Timestamp;
use jiff::civil::{Date, Time};
use serde::{Deserialize, Serialize};

use crate::calendar::{Day, DayPlan, Direction, Schedule};
use crate::departures::{self, NextDepartures};
use crate::history::History;
use crate::journal::Journal;
use crate::leaving::Refusal;
use crate::line_alerts::{self, LineAlerts};
use crate::profile::{Store, UsualTrains};
use crate::school::SchoolHolidays;
use crate::watch::{Snapshot, View};
use jiff::ToSpan;

/// Firebase tokens are a few hundred characters long.
const MAX_TOKEN_LENGTH: usize = 4096;
/// Journeys are named in a few dozen characters.
const MAX_JOURNEY_LENGTH: usize = 256;

/// `GET /settings`, in the shape fixed by `fixtures/api/settings.json`.
#[derive(Serialize)]
struct SettingsView {
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

/// Why a change is refused, in the shape fixed by `fixtures/api/pick-refused.json`
/// and `fixtures/api/day-refused.json`.
#[derive(Serialize)]
struct Refused<R> {
    refused: R,
}

#[derive(Deserialize)]
struct Month {
    /// As in "2026-11".
    month: String,
}

/// `GET /calendar`, in the shape fixed by `fixtures/api/calendar.json`.
#[derive(Serialize)]
struct MonthView {
    days: Vec<DayView>,
}

#[derive(Serialize)]
struct DayView {
    #[serde(flatten)]
    plan: DayPlan,
    /// The name of the school holidays the day is in, if any.
    school_holiday: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetDay {
    date: Date,
    /// The last day set the same, included; without it, `date` only, as the
    /// first app with the calendar sends it.
    #[serde(default)]
    through: Option<Date>,
    /// What the days are set to. Required, but null puts them back as planned.
    #[serde(deserialize_with = "Option::deserialize")]
    kind: Option<Day>,
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
    school: Arc<SchoolHolidays>,
}

impl FromRef<Shared> for Arc<SchoolHolidays> {
    fn from_ref(shared: &Shared) -> Self {
        shared.school.clone()
    }
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
    pub school: Arc<SchoolHolidays>,
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
        school,
    } = sources;
    Router::new()
        .route("/devices", post(register))
        .route("/settings", get(settings))
        .route("/status", get(status_now))
        .route("/history", get(sent))
        .route("/departures", get(next_departures))
        .route("/departure", put(pick_departure))
        .route("/usual-train", put(set_usual_train))
        .route("/alerts", get(alerts))
        .route("/trains", get(ended_trains))
        .route("/calendar", get(month).put(put_day))
        .layer(middleware::from_fn_with_state(Arc::new(key), authorize))
        .with_state(Shared {
            store,
            status,
            schedule: Arc::new(schedule),
            history,
            departures,
            line_alerts,
            journal,
            school,
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
    Json(SettingsView {
        usual_trains: store.profile().usual,
    })
}

/// As of the last poll; the outage is judged now, so that a stopped poll loop shows.
async fn status_now(
    State(status): State<Arc<Mutex<Snapshot>>>,
    State(store): State<Arc<Store>>,
    State(schedule): State<Arc<Schedule>>,
    State(school): State<Arc<SchoolHolidays>>,
) -> Json<View> {
    let now = Timestamp::now();
    // From the days as set now, not as of the last poll: the app reads this right after a change.
    let next = schedule.next_window(now, &store.profile());
    let school_holiday = school.on(schedule.today(now));
    Json(
        status
            .lock()
            .expect("no thread panics while holding the status")
            .view(now, next, school_holiday),
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
    let calendar = store.profile().calendar(&schedule.calendar);
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

/// Each day of a month, what it is and why.
async fn month(
    State(store): State<Arc<Store>>,
    State(schedule): State<Arc<Schedule>>,
    State(school): State<Arc<SchoolHolidays>>,
    Query(month): Query<Month>,
) -> Response {
    // Exactly a year and a month: "2026-11".
    let first = match month.month.len() {
        7 => format!("{}-01", month.month).parse::<Date>().ok(),
        _ => None,
    };
    let Some(first) = first else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let calendar = store.profile().calendar(&schedule.calendar);
    let holidays = school.known();
    let days = (0..i64::from(first.days_in_month()))
        .map(|offset| {
            let date = first + offset.days();
            DayView {
                plan: calendar.day(date),
                school_holiday: crate::school::on(&holidays, date).map(String::from),
            }
        })
        .collect();
    Json(MonthView { days }).into_response()
}

async fn put_day(
    State(store): State<Arc<Store>>,
    State(schedule): State<Arc<Schedule>>,
    Json(request): Json<SetDay>,
) -> Response {
    let today = schedule.today(Timestamp::now());
    set_day(&store, &schedule, today, request)
}

/// Sets a day from the app, or several in a row, or puts them back as planned.
fn set_day(store: &Store, schedule: &Schedule, today: Date, request: SetDay) -> Response {
    let mut refused = None;
    let last = request.through.unwrap_or(request.date);
    store.update(|profile| {
        refused = profile
            .set_days(&schedule.calendar, today, request.date, last, request.kind)
            .err();
    });
    match refused {
        None => StatusCode::NO_CONTENT.into_response(),
        Some(refused) => {
            (StatusCode::UNPROCESSABLE_ENTITY, Json(Refused { refused })).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{Day, Direction};
    use crate::config::Config;
    use crate::history::History;
    use crate::prim::Prim;
    use crate::profile::Pick;
    use crate::school::{Holiday, SchoolHolidays};
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
        school: Arc<SchoolHolidays>,
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
                // Nothing read: the tests set what is known.
                school: Arc::new(SchoolHolidays::new(
                    None,
                    "http://127.0.0.1:9",
                    jiff::tz::TimeZone::get("Europe/Paris").unwrap(),
                )),
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
                school: self.school.clone(),
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
            (Method::GET, "/status", None),
            (Method::GET, "/history", None),
            (Method::GET, "/departures", None),
            (Method::PUT, "/departure", Some(r#"{"journey":null}"#)),
            (Method::GET, "/alerts", None),
            (Method::GET, "/trains", None),
            (Method::GET, "/calendar?month=2026-11", None),
            (
                Method::PUT,
                "/calendar",
                Some(r#"{"date":"2026-11-05","kind":null}"#),
            ),
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
    async fn the_settings_are_read_only() {
        let api = Api::new();
        assert_eq!(
            api.authorized(Method::GET, "/settings", None).await,
            (
                StatusCode::OK,
                r#"{"usual_trains":{"outbound":null,"inbound":null}}"#.into()
            )
        );
        // The time away is given up for days set one by one.
        assert_eq!(
            api.authorized(
                Method::PUT,
                "/settings",
                Some(r#"{"away_through":"2026-12-24"}"#)
            )
            .await
            .0,
            StatusCode::METHOD_NOT_ALLOWED
        );
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
                r#"{{"watching":null,"checked_at":null,"next":"{next}","outage":null,"lines":[],"planned":[],"school_holiday":null}}"#
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

    #[tokio::test]
    async fn the_status_says_when_today_is_in_the_school_holidays() {
        let api = Api::new();
        let today = Timestamp::now().in_tz("Europe/Paris").unwrap().date();
        api.school.set(vec![Holiday {
            name: "Vacances de la Toussaint".into(),
            first: today,
            last: today,
        }]);
        let (_, body) = api.authorized(Method::GET, "/status", None).await;
        let status: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(status["school_holiday"], "Vacances de la Toussaint");
    }

    /// When the next window opens, as `GET /status` says, in Paris.
    async fn next_watch(api: &Api) -> jiff::Zoned {
        let (_, body) = api.authorized(Method::GET, "/status", None).await;
        let status: serde_json::Value = serde_json::from_str(&body).unwrap();
        let next: Timestamp = status["next"].as_str().unwrap().parse().unwrap();
        next.in_tz("Europe/Paris").unwrap()
    }

    #[tokio::test]
    async fn the_next_watch_follows_a_day_set_at_once() {
        // The app reads the status right after saving: it must already show the change.
        let api = Api::new();
        let before = next_watch(&api).await;
        // Without `through`, as the first app with the calendar sends it: that day only.
        let home = format!(r#"{{"date":"{}","kind":"remote"}}"#, before.date());
        assert_eq!(
            api.authorized(Method::PUT, "/calendar", Some(&home))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        let after = next_watch(&api).await;
        assert!(after.date() > before.date(), "{before} then {after}");
    }

    #[tokio::test]
    async fn a_month_answers_as_in_the_fixture() {
        let api = Api::new();
        api.store.update(|profile| {
            profile.days = BTreeMap::from([
                (date(2026, 11, 1), Day::Away),
                (date(2026, 11, 2), Day::Away),
                (date(2026, 11, 5), Day::Remote),
                (date(2026, 11, 12), Day::Office),
                (date(2026, 11, 20), Day::Away),
            ]);
        });
        api.school.set(vec![Holiday {
            name: "Vacances de la Toussaint".into(),
            first: date(2026, 10, 17),
            last: date(2026, 11, 1),
        }]);
        let (status, body) = api
            .authorized(Method::GET, "/calendar?month=2026-11", None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            fixture("calendar")
        );
    }

    #[tokio::test]
    async fn a_month_is_asked_by_its_year_and_number() {
        let api = Api::new();
        let (_, body) = api
            .authorized(Method::GET, "/calendar?month=2027-02", None)
            .await;
        let month: serde_json::Value = serde_json::from_str(&body).unwrap();
        let days = month["days"].as_array().unwrap();
        assert_eq!(days.len(), 28);
        assert_eq!(days[0]["date"], "2027-02-01");
        assert_eq!(days[27]["date"], "2027-02-28");
        for uri in [
            "/calendar",
            "/calendar?month=2026-13",
            "/calendar?month=2026-11-05",
            "/calendar?month=2026-1",
            "/calendar?month=+2026-11",
            "/calendar?month=november",
        ] {
            assert_eq!(
                api.authorized(Method::GET, uri, None).await.0,
                StatusCode::BAD_REQUEST,
                "{uri}"
            );
        }
    }

    #[tokio::test]
    async fn a_day_is_set_then_put_back_as_planned() {
        let api = Api::new();
        let schedule = Config::parse(include_str!("../config.example.toml"))
            .unwrap()
            .schedule();
        let today = date(2026, 11, 2);
        let set = |kind| {
            set_day(
                &api.store,
                &schedule,
                today,
                SetDay {
                    date: date(2026, 11, 5),
                    through: None,
                    kind,
                },
            )
            .status()
        };
        assert_eq!(set(Some(Day::Remote)), StatusCode::NO_CONTENT);
        assert_eq!(
            api.store.profile().days,
            BTreeMap::from([(date(2026, 11, 5), Day::Remote)])
        );
        let (_, body) = api
            .authorized(Method::GET, "/calendar?month=2026-11", None)
            .await;
        let month: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            month["days"][4],
            serde_json::json!({"date": "2026-11-05", "kind": "remote", "why": "set", "school_holiday": null})
        );
        assert_eq!(set(None), StatusCode::NO_CONTENT);
        assert_eq!(api.store.profile().days, BTreeMap::new());
    }

    #[tokio::test]
    async fn days_off_are_set_through_a_day() {
        let api = Api::new();
        let schedule = Config::parse(include_str!("../config.example.toml"))
            .unwrap()
            .schedule();
        let set = |from, through| {
            set_day(
                &api.store,
                &schedule,
                date(2026, 11, 2),
                SetDay {
                    date: date(2026, 11, from),
                    through: Some(date(2026, 11, through)),
                    kind: Some(Day::Away),
                },
            )
        };
        assert_eq!(set(5, 9).status(), StatusCode::NO_CONTENT);
        assert_eq!(
            api.store.profile().days,
            (5..=9)
                .map(|day| (date(2026, 11, day), Day::Away))
                .collect()
        );
        let backwards = set(9, 5);
        assert_eq!(backwards.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = backwards.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, r#"{"refused":"backwards"}"#);
    }

    #[tokio::test]
    async fn a_day_gone_by_or_too_far_ahead_is_refused() {
        let api = Api::new();
        let put = |body: &'static str| api.authorized(Method::PUT, "/calendar", Some(body));
        let (status, body) = put(r#"{"date":"2020-01-06","kind":"remote"}"#).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            fixture("day-refused")
        );
        assert_eq!(
            put(r#"{"date":"2100-01-04","kind":null}"#).await,
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                r#"{"refused":"too_far"}"#.into()
            )
        );
        for body in [
            r#"{"date":"2026-11-05","kind":"off"}"#,
            r#"{"date":"2026-11-05","through":"soon","kind":null}"#,
            r#"{"date":"2026-11-05"}"#,
            r#"{"kind":null}"#,
            r#"{"date":"2026-13-01","kind":null}"#,
            r#"{"date":"2026-11-05","kind":null,"other":1}"#,
        ] {
            assert_eq!(
                put(body).await.0,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{body}"
            );
        }
        assert_eq!(api.store.profile().days, BTreeMap::new());
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
        api.store.update(|profile| {
            profile.days = BTreeMap::from([(date(2026, 11, 3), Day::Away)]);
        });
        let response = pick(
            &api.store,
            &departures,
            &config.schedule(),
            Some("j2".into()),
            now,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(api.store.profile().pick, Some(picked.clone()));
        // At home that day, set from the app: refused too.
        api.store.update(|profile| {
            profile.days = BTreeMap::from([(date(2026, 11, 3), Day::Remote)]);
        });
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
    async fn other_methods_and_paths_are_refused() {
        let api = Api::new();
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
