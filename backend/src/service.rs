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

use jiff::civil::{Date, DateTime};
use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::alerts::{Commute, Notification, State, WatchedLeg, decide};
use crate::api;
use crate::calendar::{Direction, Phase, phase, watched_direction};
use crate::config::Config;
use crate::departures::{NextDepartures, leg_stops, monitoring_ref, one_way};
use crate::history::{self, Entry, History};
use crate::journal::Journal;
use crate::leaving::{Aim, Leaving, remind, reminded_leg};
use crate::line_alerts::LineAlerts;
use crate::prim::Prim;
use crate::prim::PrimError;
use crate::profile::{Profile, Store};
use crate::push::{self, PushError, Sender};
use crate::relevance::{Leg, Route};
use crate::school::{SCHOOL_URL, SchoolHolidays};
use crate::trains::{Seen, Trains, follow};
use crate::watch::{Cause, Health, LineStatus, Snapshot};

const STATE_FILE: &str = "state.json";
const PRIM_URL: &str = "https://prim.iledefrance-mobilites.fr/marketplace";
const FCM_URL: &str = "https://fcm.googleapis.com";
const ROUTES_FILE: &str = "routes.json";
const PROFILE_FILE: &str = "profile.json";
const HEALTH_FILE: &str = "health.json";
const HISTORY_FILE: &str = "history.json";
const TRAINS_FILE: &str = "trains.json";
const LEAVING_FILE: &str = "leaving.json";
const JOURNAL_FILE: &str = "journal.jsonl";

pub struct Service {
    config: Config,
    prim: Prim,
    sender: Sender,
    store: Arc<Store>,
    data_dir: PathBuf,
    state: State,
    trains: Trains,
    leaving: Leaving,
    health: Health,
    status: Arc<Mutex<Snapshot>>,
    history: Arc<History>,
    journal: Arc<Journal>,
    departures: Arc<NextDepartures>,
    line_alerts: Arc<LineAlerts>,
    routes: BTreeMap<String, Vec<Route>>,
    routes_day: Option<Date>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    /// Not an office day, or outside the watch windows.
    Idle,
    /// Before a window: trains followed, nothing notified.
    Observing,
    /// No device registered yet: nothing decided, so that a phone registering
    /// during a window still hears about what is under way.
    NoDevice,
    /// PRIM could not be read: nothing decided, nothing sent.
    Unavailable(String),
    /// Every notification decided was sent (possibly none).
    Sent(usize),
    /// A send failed: the state stays as it was, so the next poll tries again.
    NotSent(String),
    /// The line notifications decided were sent, but the trains could not be
    /// read: the check is not complete.
    TrainsUnavailable(usize, String),
}

/// What `main` runs: the poll loop, and the API that shares its profile.
pub struct Startup {
    pub service: Service,
    pub api: axum::Router,
    /// For the caller to read again from time to time; the API answers with what was last read.
    pub school: Arc<SchoolHolidays>,
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
        let routes: BTreeMap<String, Vec<Route>> =
            load(&data_dir.join(ROUTES_FILE)).unwrap_or_default();
        let line_alerts = LineAlerts::new(&config, prim.clone());
        line_alerts.set_routes(routes.clone());
        Self {
            state: load(&data_dir.join(STATE_FILE)).unwrap_or_default(),
            trains: load(&data_dir.join(TRAINS_FILE)).unwrap_or_default(),
            leaving: load(&data_dir.join(LEAVING_FILE)).unwrap_or_default(),
            health: load(&data_dir.join(HEALTH_FILE)).unwrap_or_default(),
            status: Arc::default(),
            history: Arc::new(History::open(&data_dir.join(HISTORY_FILE))),
            journal: Arc::new(Journal::open(&data_dir.join(JOURNAL_FILE))),
            departures: Arc::new(NextDepartures::new(&config, prim.clone(), routes.clone())),
            line_alerts: Arc::new(line_alerts),
            routes,
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
    /// (default `/data`), `PRIM_URL`, `FCM_URL` and `SCHOOL_URL`.
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
        let school = Arc::new(SchoolHolidays::new(
            config.school_holidays.clone(),
            &var("SCHOOL_URL").unwrap_or(SCHOOL_URL.into()),
            config.time_zone(),
        ));
        let service = Self::new(config, prim, sender, store.clone(), &data_dir);
        Ok(Startup {
            api: api::router(
                api::Sources {
                    store,
                    status: service.status(),
                    schedule,
                    history: service.history(),
                    departures: service.departures(),
                    line_alerts: service.line_alerts(),
                    journal: service.journal(),
                    school: school.clone(),
                },
                app_key,
            ),
            service,
            school,
        })
    }

    /// What was sent, and windows that closed on a disruption.
    pub fn history(&self) -> Arc<History> {
        self.history.clone()
    }

    /// How each train followed went.
    pub fn journal(&self) -> Arc<Journal> {
        self.journal.clone()
    }

    /// The next departures, read when the app asks, on the stops of the routes.
    pub fn departures(&self) -> Arc<NextDepartures> {
        self.departures.clone()
    }

    /// The disruptions of the lines, as the poll loop last read them or when the app asks.
    pub fn line_alerts(&self) -> Arc<LineAlerts> {
        self.line_alerts.clone()
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
            &profile.calendar(&self.config.calendar),
            &self.config.windows,
        );
        format!(
            "local time {}, watching {direction:?}, {} day(s) set, {} device(s)",
            now.to_zoned(time_zone).strftime("%A %Y-%m-%d %H:%M %Z"),
            profile.days.len(),
            profile.devices.len()
        )
    }

    pub async fn poll(&mut self, now: Timestamp) -> Poll {
        // A copy: no lock is held while PRIM and Firebase are called.
        let profile = self.store.profile();
        let time_zone = self.config.time_zone();
        let local = now.to_zoned(time_zone.clone()).datetime();
        let calendar = profile.calendar(&self.config.calendar);
        let followed = phase(now, &time_zone, &calendar, &self.config.windows);
        let direction = match followed {
            Some((direction, Phase::Watching)) => Some(direction),
            _ => None,
        };
        let mut health = self.health.clone();
        let outcome = match direction {
            None => {
                // No "ended" will follow for lines still disrupted: the history says so.
                for (line, severity) in self.state.lines() {
                    self.history.record(Entry {
                        at: now,
                        data: history::window_closed(line, *severity, &self.config.lines),
                        delivered: 0,
                        records: vec![],
                    });
                }
                let mut next = self.state.clone();
                decide(&mut next, local, None, &self.commute(), &[]);
                self.commit(next);
                health.idle();
                match followed {
                    Some((direction, Phase::Observing)) => {
                        self.observe(now, local, direction).await
                    }
                    _ => Poll::Idle,
                }
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
        // The backups' lines too, for the app: only the commute's legs are notified.
        let lines = self.config.all_line_codes();
        let codes: Vec<&str> = lines.iter().map(String::as_str).collect();
        let disruptions = match self.prim.disruptions(&codes).await {
            Ok(disruptions) => {
                // The app's Alertes page needs no read of its own then.
                self.line_alerts.store(now, &disruptions);
                disruptions
            }
            Err(error) => {
                let until = self.window_end(local, direction);
                self.failed(now, until, Cause::Prim, profile, health).await;
                return Poll::Unavailable(format!("{error:?}"));
            }
        };
        let mut next = self.state.clone();
        let until = self.window_end(local, direction);
        let mut notifications = decide(
            &mut next,
            local,
            Some(direction),
            &self.commute(),
            &disruptions,
        );
        // Missing trains must not hold back what the lines say.
        let seen = self.read_trains(direction, now).await;
        let mut trains = self.trains.clone();
        let mut leaving = self.leaving.clone();
        if let Ok(seen) = &seen {
            let key = (local.date(), direction);
            let window = self.window_bounds(local, direction);
            notifications.extend(follow(&mut trains, key, window, now, seen, true));
            notifications.extend(self.reminders(&mut leaving, key, now, local, seen, profile));
            // Last: when anything before it cannot be sent, the poll is tried again
            // whole, and checks are not back yet anyway.
            if health.recovering() {
                notifications.push(Notification::Recovered);
            }
        }
        if let Err(error) = self.send(now, until, profile, &notifications).await {
            self.failed(now, until, Cause::Firebase, profile, health)
                .await;
            return Poll::NotSent(format!("{error:?}"));
        }
        self.commit(next);
        self.commit_trains(trains);
        if leaving != self.leaving {
            self.leaving = leaving;
            save(&self.data_dir.join(LEAVING_FILE), &self.leaving);
        }
        match seen {
            Ok(_) => {
                health.checked(now);
                Poll::Sent(notifications.len())
            }
            Err(error) => {
                self.failed(now, until, Cause::Prim, profile, health).await;
                Poll::TrainsUnavailable(notifications.len(), format!("{error:?}"))
            }
        }
    }

    /// A poll before a window: trains are followed, nothing is notified.
    async fn observe(&mut self, now: Timestamp, local: DateTime, direction: Direction) -> Poll {
        self.refresh_routes(local.date()).await;
        match self.read_trains(direction, now).await {
            Ok(seen) => {
                let mut trains = self.trains.clone();
                let key = (local.date(), direction);
                let window = self.window_bounds(local, direction);
                follow(&mut trains, key, window, now, &seen, false);
                self.commit_trains(trains);
                Poll::Observing
            }
            Err(error) => Poll::Unavailable(format!("{error:?}")),
        }
    }

    /// The stops of each train leg of `direction`: a station lists its trains one by one.
    /// The metro's platforms list the next four trains only, without their timetable.
    async fn read_trains(
        &self,
        direction: Direction,
        now: Timestamp,
    ) -> Result<Vec<Seen>, PrimError> {
        let legs = self.config.legs(direction);
        let mut seen = vec![];
        for leg in legs {
            let Some((board, alight)) = self
                .routes
                .get(&leg.line)
                .and_then(|routes| leg_stops(leg, routes))
            else {
                continue;
            };
            let stops = monitoring_ref(&board.stop_point).zip(monitoring_ref(&alight.stop_point));
            let Some((board_ref, alight_ref)) = stops.filter(|_| !one_way(&board.stop_point))
            else {
                continue;
            };
            seen.push(Seen {
                line: leg.line.clone(),
                from: board.name.clone(),
                board: self.departures.read(&board_ref, now).await?,
                arriving: self.departures.read(&alight_ref, now).await?,
                alight: alight_ref,
            });
        }
        Ok(seen)
    }

    /// The reminder to leave, for the train picked in the app today, or else
    /// the way's usual train: on the ride `reminded_leg` says, if its station's
    /// trains are read.
    fn reminders(
        &self,
        leaving: &mut Leaving,
        key: (Date, Direction),
        now: Timestamp,
        local: DateTime,
        seen: &[Seen],
        profile: &Profile,
    ) -> Vec<Notification> {
        let legs = match key.1 {
            Direction::Outbound => &self.config.outbound,
            Direction::Inbound => &self.config.inbound,
        };
        let reached = reminded_leg(legs).and_then(|(leg, reach)| {
            Some((seen.iter().find(|seen| seen.line == leg.line)?, reach))
        });
        let Some((seen, reach)) = reached else {
            return vec![];
        };
        let reach = SignedDuration::from_mins(i64::from(reach));
        let pick = profile.pick.as_ref();
        let usual = profile.usual.of(key.1);
        let aim = match (pick.filter(|pick| (pick.day, pick.direction) == key), usual) {
            (Some(pick), _) => Aim::Picked {
                journey: &pick.journey,
                leave: pick.leave,
            },
            // Leave in time for the first train from the usual one's time.
            (None, Some(usual)) => Aim::Usual(
                local
                    .date()
                    .to_datetime(usual)
                    .to_zoned(self.config.time_zone())
                    .expect("a day's local time is within range")
                    .timestamp()
                    - reach,
            ),
            (None, None) => return vec![],
        };
        remind(leaving, key, now, seen, reach, aim)
    }

    /// When the window of `direction` ends on the day of `local`.
    fn window_end(&self, local: DateTime, direction: Direction) -> Timestamp {
        self.window_bounds(local, direction).1
    }

    /// When the window of `direction` opens and ends on the day of `local`.
    fn window_bounds(&self, local: DateTime, direction: Direction) -> (Timestamp, Timestamp) {
        let window = self.config.windows.of(direction);
        let at = |time| {
            local
                .date()
                .to_datetime(time)
                .to_zoned(self.config.time_zone())
                .expect("a day's local time is within range")
                .timestamp()
        };
        (at(window.start), at(window.end))
    }

    /// Tells the phone when polls have failed for too long.
    async fn failed(
        &mut self,
        now: Timestamp,
        until: Timestamp,
        cause: Cause,
        profile: &Profile,
        health: &mut Health,
    ) {
        if health.failed(now, cause)
            && self
                .send(now, until, profile, &[Notification::Outage])
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
        until: Timestamp,
        profile: &Profile,
        notifications: &[Notification],
    ) -> Result<(), PushError> {
        for notification in notifications {
            let data = push::data(notification, &self.config.lines, until);
            let mut delivered = 0;
            for device in &profile.devices {
                match self.sender.send(now, device, &data).await {
                    Ok(()) => delivered += 1,
                    Err(PushError::Unregistered) => {
                        eprintln!(
                            "Firebase does not know a device: forgotten until the app registers again"
                        );
                        self.store.update(|profile| profile.forget(device));
                    }
                    // Tried again, it would hold back every poll to come.
                    Err(PushError::Refused(reason)) => {
                        eprintln!(
                            "Firebase refuses a {:?} notification, not sent again: {reason}",
                            data.get("type")
                        );
                    }
                    Err(error) => return Err(error),
                }
            }
            self.history.record(Entry {
                at: now,
                data,
                delivered,
                records: history::records(notification),
            });
        }
        Ok(())
    }

    /// What the app's home screen shows: the lines of the way being watched, and how polls go.
    fn publish(&self, now: Timestamp, today: Date, direction: Option<Direction>) {
        let legs = direction.map_or(&[][..], |direction| self.config.legs(direction));
        let lines = legs
            .iter()
            .map(|leg| {
                let (line, mode, color) = push::label(&leg.line, &self.config.lines);
                LineStatus {
                    line,
                    mode,
                    color,
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

    /// Keeps the trains followed, and those that ended in the journal.
    fn commit_trains(&mut self, mut next: Trains) {
        self.journal.append(&std::mem::take(&mut next.ended));
        if next != self.trains {
            self.trains = next;
            save(&self.data_dir.join(TRAINS_FILE), &self.trains);
        }
    }

    /// Once a day, even after a failure: Navitia's daily quota must last.
    async fn refresh_routes(&mut self, today: Date) {
        if self.routes_day == Some(today) {
            return;
        }
        self.routes_day = Some(today);
        for line in self.config.all_line_codes() {
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
        self.departures.set_routes(self.routes.clone());
        self.line_alerts.set_routes(self.routes.clone());
        let backups = self.config.backups.iter().flat_map(|backup| &backup.rides);
        for leg in self
            .config
            .outbound
            .iter()
            .chain(&self.config.inbound)
            .chain(backups)
        {
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
    use crate::profile::Pick;
    use crate::push::testing::account;
    use crate::trains::Outcome;
    use crate::watch::{Cause, LineStatus, Outage};
    use wiremock::matchers::{body_string_contains, method, path, query_param};
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
            for line in ["C01740", "C02000", "C03000"] {
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
    async fn nothing_is_watched_on_a_day_off() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 0).await;
        setup.store.update(|profile| {
            profile.days = std::collections::BTreeMap::from([(
                jiff::civil::date(2026, 11, 3),
                crate::calendar::Day::Away,
            )])
        });
        assert_eq!(setup.service().poll(at(MORNING)).await, Poll::Idle);
    }

    #[tokio::test]
    async fn nothing_is_watched_on_a_day_set_at_home() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 0).await;
        setup.store.update(|profile| {
            profile.days = std::collections::BTreeMap::from([(
                jiff::civil::date(2026, 11, 3),
                crate::calendar::Day::Remote,
            )])
        });
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
            // Never the real site from the tests.
            ("SCHOOL_URL".into(), "http://127.0.0.1:9".into()),
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
        // A month from now, whatever the day the test runs.
        let day = Timestamp::now().in_tz("Europe/Paris").unwrap().date() + jiff::ToSpan::days(30);
        let request = axum::http::Request::builder()
            .method("PUT")
            .uri("/calendar")
            .header("authorization", format!("Bearer {APP_KEY}"))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(format!(
                r#"{{"date":"{day}","kind":"away"}}"#
            )))
            .unwrap();
        let response = tower::ServiceExt::oneshot(startup.api, request)
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
        assert!(
            startup
                .service
                .describe(at(MORNING))
                .contains(", 1 day(s) set,")
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
            "local time Tuesday 2026-11-03 07:00 CET, watching Some(Outbound), 0 day(s) set, 3 device(s)"
        );
        assert_eq!(
            startup.service.describe(at(LATE_MORNING)),
            "local time Tuesday 2026-11-03 11:00 CET, watching None, 0 day(s) set, 3 device(s)"
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
    async fn the_end_of_an_outage_is_said_once_what_else_was_due_got_through() {
        let setup = Setup::new().await;
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .mount(&setup.server)
            .await;
        setup.prim_answers(200, true, 2).await;
        setup.firebase_answers_to("outage", 200, 1).await;
        // The alert due with it fails once: the whole poll is tried again.
        setup.firebase_answers(500, 1).await;
        setup.firebase_answers(200, 1).await;
        setup.firebase_answers_to("recovered", 200, 1).await;
        let mut service = setup.service();
        service.poll(tuesday("06:00")).await;
        service.poll(tuesday("06:10")).await;
        service.poll(tuesday("06:12")).await;
        assert_eq!(service.poll(tuesday("06:14")).await, Poll::Sent(2));
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
        let outage = status
            .lock()
            .unwrap()
            .view(tuesday("06:10"), None, None)
            .outage;
        assert_eq!(
            outage,
            Some(Outage {
                since: tuesday("06:00"),
                cause: Cause::Firebase
            })
        );
    }

    #[tokio::test]
    async fn the_alerts_page_uses_what_the_poll_just_read() {
        let setup = Setup::new().await;
        // Read once, by the poll: the page asks for nothing more.
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(200, 1).await;
        let mut service = setup.service();
        service.poll(at(MORNING)).await;
        let view = service.line_alerts().view(at(MORNING)).await;
        let lines = view.lines.unwrap();
        assert_eq!(lines[0].alerts.len(), 1, "{lines:?}");
    }

    #[tokio::test]
    async fn each_poll_publishes_what_it_saw() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(200, 1).await;
        let mut service = setup.service();
        let status = service.status();
        service.poll(at(MORNING)).await;
        let line = |line: &str, mode, color: &str, severity| LineStatus {
            line: line.into(),
            mode,
            color: Some(color.into()),
            severity,
        };
        {
            let seen = status.lock().unwrap();
            let view = seen.view(at(MORNING), None, None);
            assert_eq!(view.watching, Some(Direction::Outbound));
            assert_eq!(view.checked_at, Some(at(MORNING)));
            assert_eq!(
                view.lines,
                [
                    line("L", "train", "#7B61C9", Some("blocking")),
                    line("X", "metro", "#E07A1F", None)
                ]
            );
        }
        service.poll(at(LATE_MORNING)).await;
        let seen = status.lock().unwrap();
        let view = seen.view(at(LATE_MORNING), None, None);
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
            .view(tuesday("15:00"), None, None);
        assert_eq!(view.watching, Some(Direction::Inbound));
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

    fn history(service: &Service) -> Vec<crate::history::Entry> {
        service.history().latest(usize::MAX)
    }

    #[tokio::test]
    async fn each_notification_sent_is_recorded_with_its_records() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(200, 1).await;
        let mut service = setup.service();
        assert_eq!(service.poll(at(MORNING)).await, Poll::Sent(1));
        let entries = history(&service);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].at, at(MORNING));
        assert_eq!(entries[0].data["type"], "started");
        // Gone from the phone when the window ends, 8:15 in Paris.
        assert_eq!(entries[0].data["until"], "2026-11-03T07:15:00Z");
        assert_eq!(entries[0].delivered, 1);
        assert_eq!(entries[0].records[0].id, "d1");
    }

    #[tokio::test]
    async fn a_notification_no_phone_accepted_is_recorded_as_such() {
        // A silent loss is exactly what the history must show.
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(404, 1).await;
        let mut service = setup.service();
        service.poll(at(MORNING)).await;
        assert_eq!(history(&service)[0].delivered, 0);
    }

    #[tokio::test]
    async fn a_notification_firebase_refuses_holds_nothing_back_and_is_not_tried_again() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 2).await;
        // Refused for good: the message itself is at fault.
        setup.firebase_answers(400, 1).await;
        let mut service = setup.service();
        assert_eq!(service.poll(tuesday("06:00")).await, Poll::Sent(1));
        assert_eq!(service.poll(tuesday("06:02")).await, Poll::Sent(0));
        assert_eq!(history(&service)[0].delivered, 0);
    }

    #[tokio::test]
    async fn a_notification_that_failed_is_not_recorded() {
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(500, 1).await;
        let mut service = setup.service();
        service.poll(at(MORNING)).await;
        assert_eq!(history(&service), []);
    }

    #[tokio::test]
    async fn a_window_closing_on_a_disrupted_line_is_recorded() {
        // No "ended" follows: the history says the window closed on it.
        let setup = Setup::new().await;
        setup.prim_answers(200, true, 1).await;
        setup.firebase_answers(200, 1).await;
        let mut service = setup.service();
        service.poll(at(MORNING)).await;
        service.poll(at(LATE_MORNING)).await;
        service
            .poll(at(LATE_MORNING) + jiff::SignedDuration::from_mins(2))
            .await;
        let entries = history(&service);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].at, at(LATE_MORNING));
        assert_eq!(
            entries[0].data,
            crate::history::window_closed(
                "C01740",
                crate::disruptions::Severity::Blocking,
                &service.config.lines
            )
        );
    }

    #[tokio::test]
    async fn a_window_closing_on_lines_running_normally_is_not_recorded() {
        let setup = Setup::new().await;
        setup.prim_answers(200, false, 1).await;
        let mut service = setup.service();
        service.poll(at(MORNING)).await;
        service.poll(at(LATE_MORNING)).await;
        assert_eq!(history(&service), []);
    }

    #[tokio::test]
    async fn routes_are_read_once_a_day_and_saved() {
        let server = routes_server(200, 1).await;
        // The backup's line too, for the stops of its rides.
        Mock::given(path("/v2/navitia/lines/line%3AIDFM%3AC03000/routes"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "routes": [] })),
            )
            .expect(1)
            .mount(&server)
            .await;
        let setup = Setup::with(server).await;
        setup.prim_answers(200, false, 2).await;
        let mut service = setup.service();
        assert_eq!(service.poll(at(MORNING)).await, Poll::Sent(0));
        assert_eq!(service.poll(at(TWO_MINUTES_LATER)).await, Poll::Sent(0));
        let saved = std::fs::read_to_string(setup.dir.path().join("routes.json")).unwrap();
        assert!(saved.contains("stop_area:IDFM:71250"), "{saved}");
    }

    #[tokio::test]
    async fn a_backup_line_is_read_for_its_backup_but_never_notified() {
        let setup = Setup::new().await;
        let feed = serde_json::json!({
            "disruptions": [{
                "id": "y1",
                "applicationPeriods": [{ "begin": "20261103T064433", "end": "20261103T093000" }],
                "lastUpdate": "20261103T064500",
                "cause": "PERTURBATION",
                "severity": "BLOQUANTE",
                "title": "Métro Y : trafic interrompu",
                "message": "<p>Le trafic est interrompu.</p>"
            }],
            "lines": [{ "id": "line:IDFM:C03000", "impactedObjects": [{ "type": "line", "id": "line:IDFM:C03000", "name": "Y", "disruptionIds": ["y1"] }] }]
        });
        Mock::given(method("GET"))
            .and(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(feed))
            .expect(1)
            .mount(&setup.server)
            .await;
        let mut service = setup.service();
        assert_eq!(service.poll(at(MORNING)).await, Poll::Sent(0));
        // Kept for the backups, without reading the feed again.
        let kept = service.line_alerts().disruptions(at(MORNING)).await;
        assert_eq!(kept.unwrap()[0].lines, ["C03000"]);
    }

    /// Line L between two train stations, which PRIM lists stop by stop, both
    /// ways; no route for the metro.
    async fn train_routes_server() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(path("/v2/navitia/lines/line%3AIDFM%3AC01740/routes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "routes": [{ "id": "route:IDFM:A" }, { "id": "route:IDFM:B" }] }),
            ))
            .mount(&server)
            .await;
        let aubepines = ("100", "71100", "Les Aubépines");
        let centrale = ("200", "71370", "Gare Centrale");
        for (route, stops) in [("A", [aubepines, centrale]), ("B", [centrale, aubepines])] {
            let rows: Vec<_> = stops
                .iter()
                .map(|(place, area, name)| {
                    serde_json::json!({ "stop_point": {
                        "id": format!("stop_point:IDFM:monomodalStopPlace:{place}"),
                        "name": name,
                        "stop_area": { "id": format!("stop_area:IDFM:{area}") }
                    } })
                })
                .collect();
            Mock::given(path(format!(
                "/v2/navitia/routes/route%3AIDFM%3A{route}/route_schedules"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "route_schedules": [{ "table": { "rows": rows } }] }),
            ))
            .mount(&server)
            .await;
        }
        Mock::given(path("/v2/navitia/lines/line%3AIDFM%3AC02000/routes"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "routes": [] })),
            )
            .mount(&server)
            .await;
        server
    }

    /// A stop listing `journeys` of line L, leaving at 7:52 in Paris and ending at Gare Centrale.
    fn stop_list(journeys: &[&str]) -> serde_json::Value {
        let trains: Vec<_> = journeys.iter().map(|journey| (*journey, "06:52")).collect();
        timed_list(200, &trains)
    }

    /// A stop listing trains of line L by journey and departure time on Tuesday,
    /// UTC, ending at the stop place `end`.
    fn timed_list(end: u32, trains: &[(&str, &str)]) -> serde_json::Value {
        let visits: Vec<_> = trains
            .iter()
            .map(|(journey, time)| {
                let time = format!("2026-11-03T{time}:00Z");
                serde_json::json!({
                    "RecordedAtTime": "2026-11-03T04:00:00Z",
                    "MonitoredVehicleJourney": {
                        "LineRef": { "value": "STIF:Line::C01740:" },
                        "FramedVehicleJourneyRef": { "DatedVehicleJourneyRef": journey },
                        "DestinationRef": { "value": format!("STIF:StopArea:SP:{end}:") },
                        "DestinationName": [{ "value": "Gare Centrale" }],
                        "MonitoredCall": {
                            "AimedDepartureTime": time,
                            "ExpectedDepartureTime": time,
                            "DepartureStatus": "onTime"
                        }
                    }
                })
            })
            .collect();
        serde_json::json!({ "Siri": { "ServiceDelivery": { "StopMonitoringDelivery": [{ "MonitoredStopVisit": visits }] } } })
    }

    #[tokio::test]
    async fn a_train_gone_before_the_window_is_notified_when_it_opens() {
        let setup = Setup::with(train_routes_server().await).await;
        setup.prim_answers(200, false, 2).await;
        // The board stop lists the train once, then no more.
        Mock::given(path("/stop-monitoring"))
            .and(query_param("MonitoringRef", "STIF:StopArea:SP:100:"))
            .respond_with(ResponseTemplate::new(200).set_body_json(stop_list(&["j1"])))
            .up_to_n_times(1)
            .mount(&setup.server)
            .await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(ResponseTemplate::new(200).set_body_json(stop_list(&[])))
            .mount(&setup.server)
            .await;
        setup.firebase_answers_to("train_cancelled", 200, 1).await;
        let mut service = setup.service();
        // From 5:30 in Paris, trains are followed without notifying.
        for time in ["04:40", "04:42", "04:44", "04:46", "04:48"] {
            assert_eq!(service.poll(tuesday(time)).await, Poll::Observing);
        }
        // 6:45 in Paris: the window opens.
        assert_eq!(service.poll(tuesday("05:45")).await, Poll::Sent(1));
        // Remembered across a restart: not said again.
        assert_eq!(setup.service().poll(tuesday("05:47")).await, Poll::Sent(0));
    }

    #[tokio::test]
    async fn trains_that_ended_are_kept_in_the_journal_once_their_poll_is_done() {
        let setup = Setup::with(train_routes_server().await).await;
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(feed(false)))
            .up_to_n_times(1)
            .mount(&setup.server)
            .await;
        setup.prim_answers(200, true, 3).await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(timed_list(200, &[("j1", "06:00")])),
            )
            .mount(&setup.server)
            .await;
        setup.firebase_answers(500, 1).await;
        setup.firebase_answers(200, 1).await;
        let mut service = setup.service();
        let journal = service.journal();
        // 6:50 in Paris: the 7:00 is followed.
        assert_eq!(service.poll(tuesday("05:50")).await, Poll::Sent(0));
        // Half an hour after it left, it ended; but the line's alert cannot be sent:
        // the poll is tried again whole, and nothing is kept yet.
        let failed = service.poll(tuesday("06:30")).await;
        assert!(format!("{failed:?}").starts_with("NotSent"), "{failed:?}");
        assert_eq!(journal.latest(5), []);
        assert_eq!(service.poll(tuesday("06:32")).await, Poll::Sent(1));
        // Still listed, long gone: not followed again, nor kept twice.
        assert_eq!(service.poll(tuesday("06:34")).await, Poll::Sent(0));
        let kept = journal.latest(5);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(
            (kept[0].train.aimed, kept[0].outcome),
            (tuesday("06:00"), Outcome::Ran)
        );
    }

    #[tokio::test]
    async fn the_trains_of_a_window_are_kept_when_the_next_one_is_observed() {
        let setup = Setup::with(train_routes_server().await).await;
        setup.prim_answers(200, false, 1).await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(timed_list(200, &[("j1", "07:10")])),
            )
            .mount(&setup.server)
            .await;
        let mut service = setup.service();
        // 8:00 in Paris on Tuesday: the 8:10 is followed until the window ends.
        assert_eq!(service.poll(tuesday("07:00")).await, Poll::Sent(0));
        assert_eq!(service.journal().latest(5), []);
        // Thursday at 5:40 in Paris, the next office day: its morning's trains are
        // observed, and Tuesday's end.
        let thursday: Timestamp = "2026-11-05T04:40:00Z".parse().unwrap();
        assert_eq!(service.poll(thursday).await, Poll::Observing);
        let kept = service.journal().latest(5);
        assert_eq!(
            kept.iter()
                .map(|ended| (ended.day, ended.direction))
                .collect::<Vec<_>>(),
            [(jiff::civil::date(2026, 11, 3), Direction::Outbound)]
        );
    }

    #[tokio::test]
    async fn the_reminder_to_leave_comes_a_few_minutes_before() {
        let setup = Setup::with(train_routes_server().await).await;
        setup.prim_answers(200, false, 3).await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(ResponseTemplate::new(200).set_body_json(stop_list(&["j1"])))
            .mount(&setup.server)
            .await;
        setup.firebase_answers_to("leave", 200, 1).await;
        usual(&setup, Direction::Outbound, 7, 52);
        let mut service = setup.service();
        // The usual train is the 7:52 in Paris: twelve minutes to its
        // platform, so leave at 7:40, and hear about it from 7:35.
        assert_eq!(service.poll(tuesday("06:34")).await, Poll::Sent(0));
        assert_eq!(service.poll(tuesday("06:36")).await, Poll::Sent(1));
        // Remembered across a restart.
        assert_eq!(setup.service().poll(tuesday("06:38")).await, Poll::Sent(0));
    }

    /// Sets the usual train of the way `direction`, by its time in Paris.
    fn usual(setup: &Setup, direction: Direction, hour: i8, minute: i8) {
        setup.store.update(|profile| {
            profile
                .usual
                .set(direction, Some(jiff::civil::time(hour, minute, 0, 0)));
        });
    }

    #[tokio::test]
    async fn without_a_usual_train_no_reminder_is_given() {
        let setup = Setup::with(train_routes_server().await).await;
        setup.prim_answers(200, false, 2).await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(ResponseTemplate::new(200).set_body_json(stop_list(&["j1"])))
            .mount(&setup.server)
            .await;
        let mut service = setup.service();
        assert_eq!(service.poll(tuesday("06:36")).await, Poll::Sent(0));
        // The evening's usual train is not the morning's.
        usual(&setup, Direction::Inbound, 7, 52);
        assert_eq!(service.poll(tuesday("06:38")).await, Poll::Sent(0));
    }

    /// Picks the train `journey` of Tuesday's way `direction`, to leave at `leave` UTC.
    fn pick(setup: &Setup, direction: Direction, journey: &str, leave: &str) {
        setup.store.update(|profile| {
            profile.pick = Some(Pick {
                day: jiff::civil::date(2026, 11, 3),
                direction,
                journey: journey.into(),
                leave: tuesday(leave),
            });
        });
    }

    #[tokio::test]
    async fn the_train_picked_today_is_reminded_rather_than_the_usual_one() {
        let setup = Setup::with(train_routes_server().await).await;
        setup.prim_answers(200, false, 3).await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(timed_list(200, &[("j1", "06:52"), ("j2", "07:07")])),
            )
            .mount(&setup.server)
            .await;
        setup.firebase_answers_to("leave", 200, 1).await;
        usual(&setup, Direction::Outbound, 7, 52);
        pick(&setup, Direction::Outbound, "j2", "06:55");
        let mut service = setup.service();
        // The 7:52 in Paris would be reminded from 7:35: the 8:07 is, from 7:50.
        assert_eq!(service.poll(tuesday("06:36")).await, Poll::Sent(0));
        assert_eq!(service.poll(tuesday("06:50")).await, Poll::Sent(1));
        assert_eq!(service.poll(tuesday("06:52")).await, Poll::Sent(0));
    }

    #[tokio::test]
    async fn without_a_usual_train_a_picked_one_is_reminded() {
        let setup = Setup::with(train_routes_server().await).await;
        setup.prim_answers(200, false, 3).await;
        // The way home boards at Gare Centrale for Les Aubépines, 25 minutes away.
        Mock::given(path("/stop-monitoring"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(timed_list(100, &[("j1", "15:45")])),
            )
            .mount(&setup.server)
            .await;
        setup.firebase_answers_to("leave", 200, 1).await;
        let mut service = setup.service();
        // The other way's pick is no pick.
        pick(&setup, Direction::Outbound, "j1", "15:20");
        assert_eq!(service.poll(tuesday("15:15")).await, Poll::Sent(0));
        pick(&setup, Direction::Inbound, "j1", "15:20");
        assert_eq!(service.poll(tuesday("15:16")).await, Poll::Sent(1));
        assert_eq!(service.poll(tuesday("15:18")).await, Poll::Sent(0));
    }

    #[tokio::test]
    async fn trains_that_cannot_be_read_hold_no_alert_back_but_fail_the_check() {
        let setup = Setup::with(train_routes_server().await).await;
        setup.prim_answers(200, true, 2).await;
        // The board stop fails once, then answers; the alight stop always fails.
        let board = || {
            Mock::given(path("/stop-monitoring"))
                .and(query_param("MonitoringRef", "STIF:StopArea:SP:100:"))
        };
        board()
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&setup.server)
            .await;
        board()
            .respond_with(ResponseTemplate::new(200).set_body_json(stop_list(&["j1"])))
            .mount(&setup.server)
            .await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&setup.server)
            .await;
        setup.firebase_answers(200, 1).await;
        setup.firebase_answers_to("outage", 200, 1).await;
        let mut service = setup.service();
        // Before the window, nothing is watched yet: no outage counted.
        assert_eq!(
            service.poll(tuesday("04:40")).await,
            Poll::Unavailable("Status(503)".into())
        );
        assert_eq!(
            service.poll(tuesday("06:00")).await,
            Poll::TrainsUnavailable(1, "Status(503)".into())
        );
        service.poll(tuesday("06:10")).await;
        let outage = service
            .status()
            .lock()
            .unwrap()
            .view(tuesday("06:10"), None, None)
            .outage;
        assert_eq!(
            outage,
            Some(Outage {
                since: tuesday("06:00"),
                cause: Cause::Prim
            })
        );
    }

    #[tokio::test]
    async fn the_departures_follow_the_routes_saved_then_read() {
        let setup = Setup::with(routes_server(200, 1).await).await;
        // Read for the morning's backup by the first view, then by the poll.
        setup.prim_answers(200, false, 2).await;
        let saved = r#"{"C01740":[{"stops":[{"stop_point":"sp:1","stop_area":"stop_area:IDFM:71100","name":"Saved"},{"stop_point":"sp:2","stop_area":"stop_area:IDFM:71370","name":"Saved too"}]}]}"#;
        std::fs::write(setup.dir.path().join("routes.json"), saved).unwrap();
        let mut service = setup.service();
        let departures = service.departures();
        let feed = service.line_alerts();
        let from = async |now| {
            departures
                .view(at(now), None, &Default::default(), &feed)
                .await
                .legs[0]
                .from
                .clone()
        };
        assert_eq!(from(MORNING).await.as_deref(), Some("Saved"));
        service.poll(at(MORNING)).await;
        assert_eq!(from(TWO_MINUTES_LATER).await.as_deref(), Some("71100"));
    }

    #[tokio::test]
    async fn the_alerts_page_places_records_on_the_routes_saved_from_the_start() {
        let setup = Setup::new().await;
        let saved = r#"{"C01740":[{"stops":[{"stop_point":"sp:1","stop_area":"stop_area:IDFM:71100","name":"1"},{"stop_point":"sp:2","stop_area":"stop_area:IDFM:71370","name":"2"},{"stop_point":"sp:3","stop_area":"stop_area:IDFM:71390","name":"3"}]},{"stops":[{"stop_point":"sp:3","stop_area":"stop_area:IDFM:71390","name":"3"},{"stop_point":"sp:2","stop_area":"stop_area:IDFM:71370","name":"2"},{"stop_point":"sp:1","stop_area":"stop_area:IDFM:71100","name":"1"}]}]}"#;
        std::fs::write(setup.dir.path().join("routes.json"), saved).unwrap();
        // A section beyond both rides of the line: off the way, once placed.
        let mut beyond = feed(true);
        beyond["disruptions"][0]["impactedSections"] = serde_json::json!([{
            "lineId": "line:IDFM:C01740",
            "from": { "type": "stop_area", "id": "stop_area:IDFM:71390", "name": "3" },
            "to": { "type": "stop_area", "id": "stop_area:IDFM:71395", "name": "4" }
        }]);
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(beyond))
            .expect(1)
            .mount(&setup.server)
            .await;
        // Before any poll has read the routes again.
        let view = setup.service().line_alerts().view(at(MORNING)).await;
        let lines = view.lines.unwrap();
        assert!(!lines[0].alerts[0].on_way, "{lines:?}");
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
