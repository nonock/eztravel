//! Every disruption of the commute's lines that has not ended, for the app's
//! Alertes page: now first, worst first, then what is coming.
//!
//! The feed (`disruptions_bulk`) costs one of 1 000 calls a day, shared with the
//! poll loop, and says nothing of the quota left. So the poll loop's own reads are
//! kept here, and the app's asks read again at most every `MAX_AGE`, failed reads
//! included, one at a time.

use std::collections::BTreeMap;
use std::sync::Mutex;

use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use serde::Serialize;

use crate::config::Config;
use crate::disruptions::{Cause, Disruption, Period, Severity};
use crate::prim::Prim;
use crate::push::{self, LineLabel};
use crate::relevance::{Leg, Route, affects};

/// The app's asks read the feed again when the last read is older than this.
pub const MAX_AGE: SignedDuration = SignedDuration::from_mins(5);

/// The answer to `GET /alerts`, in the shape fixed by `fixtures/api/alerts.json`.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct View {
    /// In the order of the configuration; none when the feed cannot be read.
    pub lines: Option<Vec<LineView>>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct LineView {
    pub line: String,
    pub mode: &'static str,
    pub color: Option<String>,
    pub alerts: Vec<Alert>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Alert {
    pub id: String,
    /// `blocking`, `disrupted` or `information`; an unknown severity is a disruption.
    pub severity: &'static str,
    /// `works`, `incident`, `information` or `unknown`.
    pub cause: &'static str,
    pub title: String,
    /// The operator's whole text, as plain text.
    pub text: String,
    /// Under way now; otherwise to come.
    pub active: bool,
    /// The current period, or else the next one: works can list dozens.
    pub begin: Timestamp,
    pub end: Timestamp,
    /// It touches a leg of the commute: the poll loop would notify it.
    pub on_way: bool,
    pub updated: Timestamp,
}

/// The feed as last read, for the poll loop and the app to share.
pub struct LineAlerts {
    prim: Prim,
    /// The lines of the commute, as listed on the Alertes page.
    lines: Vec<LineLabel>,
    /// Those of the backups too, whose disruptions the backups need.
    codes: Vec<String>,
    legs: Vec<Leg>,
    time_zone: TimeZone,
    routes: Mutex<BTreeMap<String, Vec<Route>>>,
    /// When it was read, and what it gave: none when it could not be read.
    last: Mutex<Option<(Timestamp, Option<Vec<Disruption>>)>>,
    /// Held while an ask reads the feed, so that two asks never both do.
    reading: tokio::sync::Mutex<()>,
}

impl LineAlerts {
    pub fn new(config: &Config, prim: Prim) -> Self {
        Self {
            prim,
            lines: config
                .lines
                .iter()
                .filter(|label| config.line_codes().contains(&label.code))
                .cloned()
                .collect(),
            codes: config.all_line_codes(),
            legs: config
                .outbound
                .iter()
                .chain(&config.inbound)
                .cloned()
                .collect(),
            time_zone: config.time_zone(),
            routes: Mutex::default(),
            last: Mutex::default(),
            reading: tokio::sync::Mutex::default(),
        }
    }

    /// The routes say which disruptions touch the commute; the poll loop refreshes them daily.
    pub fn set_routes(&self, routes: BTreeMap<String, Vec<Route>>) {
        *self
            .routes
            .lock()
            .expect("no thread panics while holding the routes") = routes;
    }

    /// The poll loop's own read: the app's next ask needs no other.
    pub fn store(&self, now: Timestamp, disruptions: &[Disruption]) {
        *self
            .last
            .lock()
            .expect("no thread panics while holding the alerts") =
            Some((now, Some(disruptions.to_vec())));
    }

    pub async fn view(&self, now: Timestamp) -> View {
        View {
            lines: self
                .disruptions(now)
                .await
                .map(|disruptions| self.shape(now, &disruptions)),
        }
    }

    /// The disruptions of the commute's and the backups' lines, as last read:
    /// none when the feed cannot be read.
    pub async fn disruptions(&self, now: Timestamp) -> Option<Vec<Disruption>> {
        let _reading = self.reading.lock().await;
        let kept = self
            .last
            .lock()
            .expect("no thread panics while holding the alerts")
            .clone()
            .filter(|(read_at, _)| now.duration_since(*read_at) < MAX_AGE);
        match kept {
            Some((_, disruptions)) => disruptions,
            None => {
                let codes: Vec<&str> = self.codes.iter().map(String::as_str).collect();
                let read = self
                    .prim
                    .disruptions(&codes)
                    .await
                    .inspect_err(|error| eprintln!("alerts unavailable: {error:?}"))
                    .ok();
                *self
                    .last
                    .lock()
                    .expect("no thread panics while holding the alerts") =
                    Some((now, read.clone()));
                read
            }
        }
    }

    fn shape(&self, now: Timestamp, disruptions: &[Disruption]) -> Vec<LineView> {
        let local = now.to_zoned(self.time_zone.clone()).datetime();
        let routes = self
            .routes
            .lock()
            .expect("no thread panics while holding the routes");
        self.lines
            .iter()
            .map(|label| {
                let line_routes = routes.get(&label.code).map_or(&[][..], Vec::as_slice);
                let mut alerts: Vec<(u8, Alert)> = disruptions
                    .iter()
                    .filter(|disruption| disruption.lines.contains(&label.code))
                    .filter_map(|disruption| {
                        let (active, period) = shown_period(&disruption.periods, local)?;
                        let (rank, severity) = match disruption.severity {
                            Severity::Blocking => (0, "blocking"),
                            Severity::Information => (2, "information"),
                            Severity::Disrupted | Severity::Unknown => (1, "disrupted"),
                        };
                        let on_way = self
                            .legs
                            .iter()
                            .filter(|leg| leg.line == label.code)
                            .any(|leg| affects(disruption, leg, line_routes));
                        Some((
                            rank,
                            Alert {
                                id: disruption.id.clone(),
                                severity,
                                cause: cause_name(disruption.cause),
                                title: disruption.title.clone(),
                                text: push::plain_text(&disruption.message),
                                active,
                                begin: self.instant(period.begin),
                                end: self.instant(period.end),
                                on_way,
                                updated: self.instant(disruption.updated),
                            },
                        ))
                    })
                    .collect();
                alerts.sort_by_key(|(rank, alert)| (!alert.active, *rank, alert.begin));
                let (name, mode) = push::label(&label.code, &self.lines);
                LineView {
                    line: name,
                    mode,
                    color: label.color.clone(),
                    alerts: alerts.into_iter().map(|(_, alert)| alert).collect(),
                }
            })
            .collect()
    }

    fn instant(&self, local: DateTime) -> Timestamp {
        local
            .to_zoned(self.time_zone.clone())
            .expect("a network local time is within range")
            .timestamp()
    }
}

/// The cause as the app reads it.
fn cause_name(cause: Cause) -> &'static str {
    match cause {
        Cause::Works => "works",
        Cause::Incident => "incident",
        Cause::Information => "information",
        Cause::Unknown => "unknown",
    }
}

/// The period under way at `now`, or else the next one; none once all have ended.
fn shown_period(periods: &[Period], now: DateTime) -> Option<(bool, Period)> {
    let current = periods
        .iter()
        .find(|period| period.begin <= now && now < period.end);
    match current {
        Some(period) => Some((true, *period)),
        None => periods
            .iter()
            .filter(|period| period.begin > now)
            .min_by_key(|period| period.begin)
            .map(|period| (false, *period)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::departures::example::{at, routes};
    use crate::disruptions::{Place, Section};
    use jiff::civil::{date, datetime};
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const EXAMPLE: &str = include_str!("../config.example.toml");

    fn period(begin: DateTime, end: DateTime) -> Period {
        Period { begin, end }
    }

    fn place(area: &str) -> Place {
        Place {
            id: format!("stop_area:IDFM:{area}"),
            name: area.into(),
        }
    }

    /// The example's disruptions on Tuesday 3 November 2026, Paris time.
    fn disruptions() -> Vec<Disruption> {
        let day = date(2026, 11, 3);
        let disruption =
            |id: &str, line: &str, severity, cause, title: &str, message: &str| Disruption {
                id: id.into(),
                lines: vec![line.into()],
                stops: vec![],
                periods: vec![],
                updated: day.at(7, 35, 0, 0),
                cause,
                severity,
                title: title.into(),
                short_message: None,
                message: message.into(),
                sections: vec![],
            };
        let incident = Disruption {
            periods: vec![period(day.at(7, 30, 0, 0), day.at(9, 0, 0, 0))],
            sections: vec![Section {
                line: "C01740".into(),
                from: place("71100"),
                to: place("71370"),
            }],
            ..disruption(
                "incident-l",
                "C01740",
                Severity::Blocking,
                Cause::Incident,
                "Ligne L : trafic interrompu entre Les Aubépines et Gare Centrale",
                "<p>Le trafic est interrompu entre Les Aubépines et Gare Centrale jusqu&#39;à 9h.</p><p>Motif : panne d'un train.</p>",
            )
        };
        // Works on weekend evenings: the next ones are shown.
        let works = Disruption {
            periods: vec![
                period(
                    datetime(2026, 10, 31, 22, 0, 0, 0),
                    datetime(2026, 11, 1, 5, 0, 0, 0),
                ),
                period(
                    datetime(2026, 11, 14, 22, 0, 0, 0),
                    datetime(2026, 11, 15, 5, 0, 0, 0),
                ),
                period(
                    datetime(2026, 11, 7, 22, 0, 0, 0),
                    datetime(2026, 11, 8, 5, 0, 0, 0),
                ),
            ],
            updated: datetime(2026, 10, 20, 10, 0, 0, 0),
            sections: vec![Section {
                line: "C01740".into(),
                from: place("79998"),
                to: place("79999"),
            }],
            ..disruption(
                "works-l",
                "C01740",
                Severity::Information,
                Cause::Works,
                "Ligne L : travaux à Les Peupliers",
                "Les week-ends de novembre, les trains ne desservent pas Les Peupliers après 22h.",
            )
        };
        let slow = Disruption {
            periods: vec![period(day.at(7, 0, 0, 0), day.at(10, 0, 0, 0))],
            updated: day.at(7, 40, 0, 0),
            ..disruption(
                "slow-x",
                "C02000",
                Severity::Unknown,
                Cause::Unknown,
                "Métro X : trafic ralenti",
                "Le trafic est ralenti sur l'ensemble de la ligne.",
            )
        };
        let over = Disruption {
            periods: vec![period(day.at(5, 0, 0, 0), day.at(6, 0, 0, 0))],
            ..disruption(
                "over-l",
                "C01740",
                Severity::Blocking,
                Cause::Incident,
                "Fini",
                "",
            )
        };
        // Listed out of order, as the feed may.
        vec![works, over, slow, incident]
    }

    fn line_alerts(server: &str) -> LineAlerts {
        let alerts = LineAlerts::new(&Config::parse(EXAMPLE).unwrap(), Prim::new(server, "key"));
        alerts.set_routes(routes());
        alerts
    }

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!("{}/../fixtures/api/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn the_alerts_match_the_fixture() {
        // The poll loop's read: the feed is not asked again.
        let alerts = line_alerts("http://127.0.0.1:9");
        alerts.store(at("06:48:00"), &disruptions());
        let view = alerts.view(at("06:50:00")).await;
        assert_eq!(serde_json::to_value(&view).unwrap(), fixture("alerts"));
    }

    #[tokio::test]
    async fn the_feed_is_read_at_most_every_five_minutes_even_when_it_fails() {
        let server = MockServer::start().await;
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(503))
            .expect(2)
            .mount(&server)
            .await;
        let alerts = line_alerts(&server.uri());
        // Unreadable: said so, not taken as "nothing to say".
        assert_eq!(alerts.view(at("06:50:00")).await, View { lines: None });
        assert_eq!(alerts.view(at("06:54:59")).await, View { lines: None });
        alerts.view(at("06:55:00")).await;
    }

    #[tokio::test]
    async fn backup_lines_are_read_for_the_backups_but_not_listed() {
        let server = MockServer::start().await;
        let feed = serde_json::json!({
            "lastUpdatedDate": "2026-11-03T06:02:15.628Z",
            "disruptions": [{
                "id": "slow-y",
                "applicationPeriods": [{ "begin": "20261103T070000", "end": "20261103T100000" }],
                "lastUpdate": "20261103T070000",
                "cause": "PERTURBATION",
                "severity": "PERTURBEE",
                "title": "Métro Y : trafic ralenti",
                "message": "<p>Le trafic est ralenti.</p>"
            }],
            "lines": [{
                "id": "line:IDFM:C03000", "name": "Y", "shortName": "Y", "mode": "Metro",
                "impactedObjects": [
                    { "type": "line", "id": "line:IDFM:C03000", "name": "Y", "disruptionIds": ["slow-y"] }
                ]
            }]
        });
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(feed))
            .expect(1)
            .mount(&server)
            .await;
        let alerts = line_alerts(&server.uri());
        let read = alerts.disruptions(at("06:50:00")).await.unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].lines, ["C03000"]);
        // The same read: the Alertes page lists the lines of the commute only.
        let lines = alerts.view(at("06:51:00")).await.lines.unwrap();
        let names: Vec<&str> = lines.iter().map(|line| line.line.as_str()).collect();
        assert_eq!(names, ["L", "X"]);
        assert!(lines.iter().all(|line| line.alerts.is_empty()));
    }

    #[test]
    fn causes_are_named_for_the_app() {
        let names: Vec<_> = [
            Cause::Works,
            Cause::Incident,
            Cause::Information,
            Cause::Unknown,
        ]
        .into_iter()
        .map(cause_name)
        .collect();
        assert_eq!(names, ["works", "incident", "information", "unknown"]);
    }

    #[test]
    fn a_disruption_with_its_periods_over_is_left_out() {
        let now = date(2026, 11, 3).at(7, 50, 0, 0);
        assert_eq!(shown_period(&disruptions()[1].periods, now), None);
        assert_eq!(shown_period(&[], now), None);
    }
}
