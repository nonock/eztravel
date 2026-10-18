//! The API the app calls: register the phone, read and set the time away,
//! and see what the backend sees.
//!
//! Every request carries the app key (`Authorization: Bearer …`), compiled
//! into the user's own build of the app and kept out of the repository.

use std::sync::{Arc, Mutex};

use aws_lc_rs::constant_time::verify_slices_are_equal;
use axum::extract::{FromRef, Request, State};
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use jiff::Timestamp;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::calendar::Schedule;
use crate::profile::Store;
use crate::watch::{Snapshot, View};

/// Firebase tokens are a few hundred characters long.
const MAX_TOKEN_LENGTH: usize = 4096;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Last day away from the office, included. Required, but may be null.
    #[serde(deserialize_with = "Option::deserialize")]
    pub away_through: Option<Date>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Device {
    token: String,
}

/// What the handlers share with the poll loop.
#[derive(Clone)]
struct Shared {
    store: Arc<Store>,
    status: Arc<Mutex<Snapshot>>,
    schedule: Arc<Schedule>,
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

impl FromRef<Shared> for Arc<Schedule> {
    fn from_ref(shared: &Shared) -> Self {
        shared.schedule.clone()
    }
}

pub fn router(
    store: Arc<Store>,
    status: Arc<Mutex<Snapshot>>,
    schedule: Schedule,
    key: String,
) -> Router {
    Router::new()
        .route("/devices", post(register))
        .route("/settings", get(settings).put(set_settings))
        .route("/status", get(status_now))
        .layer(middleware::from_fn_with_state(Arc::new(key), authorize))
        .with_state(Shared {
            store,
            status,
            schedule: Arc::new(schedule),
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

async fn settings(State(store): State<Arc<Store>>) -> Json<Settings> {
    Json(Settings {
        away_through: store.profile().away_through,
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
    use crate::config::Config;
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
        _dir: tempfile::TempDir,
    }

    impl Api {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(Store::open(&dir.path().join("profile.json"), &[]));
            Api {
                store,
                status: Arc::default(),
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
            let response = router(
                self.store.clone(),
                self.status.clone(),
                schedule,
                KEY.into(),
            )
            .oneshot(request)
            .await
            .unwrap();
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
            (StatusCode::OK, r#"{"away_through":null}"#.into())
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
        assert_eq!(
            api.authorized(Method::GET, "/settings", None).await.1,
            r#"{"away_through":"2026-12-24"}"#
        );
        api.authorized(Method::PUT, "/settings", Some(r#"{"away_through":null}"#))
            .await;
        assert_eq!(api.store.profile().away_through, None);
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
