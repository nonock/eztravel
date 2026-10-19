//! Next departures on the legs of the commute, from PRIM's `stop-monitoring`
//! (SIRI Lite), for the home screen.
//!
//! A stop's answer lists the calls of the coming hours with their aimed and
//! expected times, in UTC. A train station (stop place) lists both ways: a
//! train is kept only if it also calls at the leg's alight stop, later. A
//! metro platform (quay) lists one way only, and its next four trains.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use jiff::civil::{Date, Time};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::calendar::WatchWindows;
use crate::config::Config;
use crate::prim::Prim;
use crate::push::{self, LineLabel};
use crate::relevance::{Leg, Route, RouteStop};

/// Departures shown per leg.
pub const SHOWN: usize = 5;
/// A stop is read again when its answer is older than this.
const MAX_AGE: SignedDuration = SignedDuration::from_secs(60);
/// A train due this long ago may still be at the platform.
const GRACE: SignedDuration = SignedDuration::from_secs(60);
/// Stops are no longer read on demand when fewer requests are left for the day:
/// an app left open must not use up the daily quota.
const RESERVE: u32 = 200;

/// One call of a vehicle at the stop asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Visit {
    /// The same for a vehicle at every stop of its journey.
    pub journey: String,
    /// Line code ("C01740").
    pub line: String,
    pub recorded_at: Timestamp,
    /// The mission code of a train ("PALO").
    pub mission: Option<String>,
    pub destination: String,
    /// Where the journey ends, as a `MonitoringRef` for a train ("STIF:StopArea:SP:…:").
    pub destination_ref: Option<String>,
    pub platform: Option<String>,
    pub aimed_departure: Option<Timestamp>,
    pub expected_departure: Option<Timestamp>,
    pub aimed_arrival: Option<Timestamp>,
    pub expected_arrival: Option<Timestamp>,
    pub status: CallStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallStatus {
    OnTime,
    Delayed,
    Cancelled,
}

/// Reads a `stop-monitoring` answer. A vehicle listed twice keeps its latest record.
pub fn parse(json: &[u8]) -> Result<Vec<Visit>, serde_json::Error> {
    let answer: Answer = serde_json::from_slice(json)?;
    let mut visits: Vec<Visit> = vec![];
    for raw in answer
        .siri
        .delivery
        .stops
        .into_iter()
        .flat_map(|stop| stop.visits)
    {
        let visit = raw.visit();
        match visits
            .iter_mut()
            .find(|known| known.journey == visit.journey)
        {
            Some(known) if known.recorded_at < visit.recorded_at => *known = visit,
            Some(_) => {}
            None => visits.push(visit),
        }
    }
    Ok(visits)
}

#[derive(Deserialize)]
struct Answer {
    #[serde(rename = "Siri")]
    siri: Siri,
}

#[derive(Deserialize)]
struct Siri {
    #[serde(rename = "ServiceDelivery")]
    delivery: Delivery,
}

#[derive(Deserialize)]
struct Delivery {
    #[serde(rename = "StopMonitoringDelivery")]
    stops: Vec<StopDelivery>,
}

#[derive(Deserialize)]
struct StopDelivery {
    #[serde(rename = "MonitoredStopVisit", default)]
    visits: Vec<RawVisit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawVisit {
    recorded_at_time: Timestamp,
    monitored_vehicle_journey: Journey,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Journey {
    line_ref: Text,
    framed_vehicle_journey_ref: Framed,
    destination_ref: Option<Text>,
    #[serde(default)]
    destination_name: Vec<Text>,
    #[serde(default)]
    journey_note: Vec<Text>,
    monitored_call: Call,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Framed {
    dated_vehicle_journey_ref: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Call {
    aimed_departure_time: Option<Timestamp>,
    expected_departure_time: Option<Timestamp>,
    aimed_arrival_time: Option<Timestamp>,
    expected_arrival_time: Option<Timestamp>,
    #[serde(default)]
    departure_status: String,
    #[serde(default)]
    arrival_status: String,
    departure_platform_name: Option<Text>,
    arrival_platform_name: Option<Text>,
}

/// SIRI Lite wraps most strings.
#[derive(Deserialize)]
struct Text {
    value: String,
}

impl RawVisit {
    fn visit(self) -> Visit {
        let journey = self.monitored_vehicle_journey;
        let call = journey.monitored_call;
        let line = journey.line_ref.value;
        // Without a departure status, a call at a train's last stop has an arrival one.
        let status = if call.departure_status.is_empty() {
            call.arrival_status
        } else {
            call.departure_status
        };
        Visit {
            journey: journey.framed_vehicle_journey_ref.dated_vehicle_journey_ref,
            line: line
                .strip_prefix("STIF:Line::")
                .and_then(|code| code.strip_suffix(':'))
                .map_or(line.clone(), String::from),
            recorded_at: self.recorded_at_time,
            mission: journey
                .journey_note
                .into_iter()
                .next()
                .map(|note| note.value),
            destination: journey
                .destination_name
                .into_iter()
                .next()
                .map(|name| name.value)
                .unwrap_or_default(),
            destination_ref: journey.destination_ref.map(|stop| stop.value),
            platform: call
                .departure_platform_name
                .or(call.arrival_platform_name)
                .map(|name| name.value)
                .filter(|name| !name.is_empty() && name != "unknown"),
            aimed_departure: call.aimed_departure_time,
            expected_departure: call.expected_departure_time,
            aimed_arrival: call.aimed_arrival_time,
            expected_arrival: call.expected_arrival_time,
            status: match status.as_str() {
                "cancelled" => CallStatus::Cancelled,
                "delayed" => CallStatus::Delayed,
                _ => CallStatus::OnTime,
            },
        }
    }
}

impl Visit {
    /// When the vehicle reaches this stop, or leaves it when that is all the answer says.
    fn reaches(&self) -> Option<Timestamp> {
        self.expected_arrival
            .or(self.aimed_arrival)
            .or(self.expected_departure)
            .or(self.aimed_departure)
    }
}

impl CallStatus {
    fn name(self) -> &'static str {
        match self {
            CallStatus::OnTime => "on_time",
            CallStatus::Delayed => "delayed",
            CallStatus::Cancelled => "cancelled",
        }
    }
}

/// The `MonitoringRef` of a Navitia stop point: a train station's stop place,
/// or a metro platform.
pub fn monitoring_ref(stop_point: &str) -> Option<String> {
    let id = stop_point.strip_prefix("stop_point:IDFM:")?;
    let number = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    match id.strip_prefix("monomodalStopPlace:") {
        Some(place) if number(place) => Some(format!("STIF:StopArea:SP:{place}:")),
        None if number(id) => Some(format!("STIF:StopPoint:Q:{id}:")),
        _ => None,
    }
}

/// Whether the stop lists one way only: a platform, not a whole station.
fn one_way(stop_point: &str) -> bool {
    monitoring_ref(stop_point).is_some_and(|stop| stop.starts_with("STIF:StopPoint:Q:"))
}

/// Where `leg` boards and alights on the first route that runs from one to the other.
pub fn leg_stops<'a>(leg: &Leg, routes: &'a [Route]) -> Option<(&'a RouteStop, &'a RouteStop)> {
    routes.iter().find_map(|route| {
        let board = route.area_index(&leg.board)?;
        let alight = route.area_index(&leg.alight)?;
        (board < alight).then(|| (&route.stops[board], &route.stops[alight]))
    })
}

/// What the app shows of one departure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Departure {
    /// None on the metro, which only gives expected times.
    pub aimed: Option<Timestamp>,
    pub expected: Timestamp,
    /// At the alight stop, when known.
    pub arrival: Option<Timestamp>,
    pub mission: Option<String>,
    pub destination: String,
    pub platform: Option<String>,
    /// `on_time`, `delayed` or `cancelled`, as the operator says.
    pub status: &'static str,
}

/// The next departures of `line` from the board stop, soonest first. With the
/// alight stop and its visits, only those that call there afterwards, or end there:
/// during an incident, a terminus drops trains from its arrivals long before they come.
pub fn select(
    line: &str,
    board: &[Visit],
    alight: Option<(&str, &[Visit])>,
    now: Timestamp,
) -> Vec<Departure> {
    let mut shown: Vec<Departure> = board
        .iter()
        .filter(|visit| visit.line == line)
        .filter_map(|visit| {
            // A call with no departure ends the journey here: nothing to board.
            let expected = visit.expected_departure.or(visit.aimed_departure)?;
            if expected < now - GRACE {
                return None;
            }
            let arrival = match alight {
                None => None,
                Some((stop, visits)) => {
                    let arrival = visits
                        .iter()
                        .find(|other| other.journey == visit.journey)
                        .and_then(Visit::reaches)
                        .filter(|arrival| *arrival > expected);
                    if arrival.is_none() && visit.destination_ref.as_deref() != Some(stop) {
                        return None;
                    }
                    arrival
                }
            };
            Some(Departure {
                aimed: visit.aimed_departure,
                expected,
                arrival,
                mission: visit.mission.clone(),
                destination: visit.destination.clone(),
                platform: visit.platform.clone(),
                status: visit.status.name(),
            })
        })
        .collect();
    shown.sort_by_key(|departure| departure.expected);
    shown.truncate(SHOWN);
    shown
}

/// The way shown: the morning one until its window ends, the evening one after.
fn shown_direction(time: Time, windows: &WatchWindows) -> &'static str {
    if time < windows.outbound.end {
        "outbound"
    } else {
        "inbound"
    }
}

/// The answer to `GET /departures`, in the shape fixed by `fixtures/api/`.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct View {
    pub direction: &'static str,
    pub legs: Vec<LegView>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct LegView {
    pub line: String,
    pub mode: &'static str,
    pub color: Option<String>,
    /// Stop names, unknown until the line's routes are.
    pub from: Option<String>,
    pub to: Option<String>,
    /// None when they cannot be read.
    pub departures: Option<Vec<Departure>>,
}

/// Reads stops when the app asks, at most once a minute each.
pub struct NextDepartures {
    prim: Prim,
    outbound: Vec<Leg>,
    inbound: Vec<Leg>,
    lines: Vec<LineLabel>,
    windows: WatchWindows,
    time_zone: TimeZone,
    routes: Mutex<BTreeMap<String, Vec<Route>>>,
    /// Held while a stop is read, so that two requests do not both read it.
    cache: tokio::sync::Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    stops: HashMap<String, (Timestamp, Vec<Visit>)>,
    /// Requests left on the UTC day of the last answer.
    quota: Option<(Date, u32)>,
}

impl NextDepartures {
    pub fn new(config: &Config, prim: Prim, routes: BTreeMap<String, Vec<Route>>) -> Self {
        Self {
            prim,
            outbound: config.outbound.clone(),
            inbound: config.inbound.clone(),
            lines: config.lines.clone(),
            windows: config.windows.clone(),
            time_zone: config.time_zone(),
            routes: Mutex::new(routes),
            cache: tokio::sync::Mutex::default(),
        }
    }

    /// The routes give the stops of each leg; the poll loop refreshes them daily.
    pub fn set_routes(&self, routes: BTreeMap<String, Vec<Route>>) {
        *self
            .routes
            .lock()
            .expect("no thread panics while holding the routes") = routes;
    }

    pub async fn view(&self, now: Timestamp) -> View {
        let direction = shown_direction(now.to_zoned(self.time_zone.clone()).time(), &self.windows);
        let legs = if direction == "outbound" {
            &self.outbound
        } else {
            &self.inbound
        };
        let stops: Vec<Option<(RouteStop, RouteStop)>> = {
            let routes = self
                .routes
                .lock()
                .expect("no thread panics while holding the routes");
            legs.iter()
                .map(|leg| {
                    let (board, alight) = leg_stops(leg, routes.get(&leg.line)?)?;
                    Some((board.clone(), alight.clone()))
                })
                .collect()
        };
        let mut cache = self.cache.lock().await;
        // Judged once per request, so that all of its legs are read or none.
        let today = now.to_zoned(TimeZone::UTC).date();
        let may_read = !cache
            .quota
            .is_some_and(|(day, left)| day == today && left < RESERVE);
        let mut views = vec![];
        for (leg, stops) in legs.iter().zip(stops) {
            let departures = match &stops {
                Some((board, alight)) => {
                    self.departures(&mut cache, may_read, &leg.line, board, alight, now)
                        .await
                }
                None => None,
            };
            let (line, mode) = push::label(&leg.line, &self.lines);
            views.push(LegView {
                line,
                mode,
                color: self
                    .lines
                    .iter()
                    .find(|label| label.code == leg.line)
                    .and_then(|label| label.color.clone()),
                from: stops.as_ref().map(|(board, _)| board.name.clone()),
                to: stops.map(|(_, alight)| alight.name),
                departures,
            });
        }
        View {
            direction,
            legs: views,
        }
    }

    async fn departures(
        &self,
        cache: &mut Cache,
        may_read: bool,
        line: &str,
        board: &RouteStop,
        alight: &RouteStop,
        now: Timestamp,
    ) -> Option<Vec<Departure>> {
        let leaving = self
            .visits(cache, may_read, &monitoring_ref(&board.stop_point)?, now)
            .await?;
        if one_way(&board.stop_point) {
            return Some(select(line, &leaving, None, now));
        }
        let stop = monitoring_ref(&alight.stop_point)?;
        let arriving = self.visits(cache, may_read, &stop, now).await?;
        Some(select(line, &leaving, Some((&stop, &arriving)), now))
    }

    /// A stop's visits, read again when older than a minute.
    async fn visits(
        &self,
        cache: &mut Cache,
        may_read: bool,
        stop: &str,
        now: Timestamp,
    ) -> Option<Vec<Visit>> {
        if let Some((read_at, visits)) = cache.stops.get(stop)
            && now.duration_since(*read_at) < MAX_AGE
        {
            return Some(visits.clone());
        }
        if !may_read {
            return None;
        }
        match self.prim.stop_visits(stop).await {
            Ok(answer) => {
                if let Some(left) = answer.remaining {
                    cache.quota = Some((now.to_zoned(TimeZone::UTC).date(), left));
                }
                cache
                    .stops
                    .insert(stop.to_string(), (now, answer.visits.clone()));
                Some(answer.visits)
            }
            Err(error) => {
                eprintln!("departures at {stop} unavailable: {error:?}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::time;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const EXAMPLE: &str = include_str!("../config.example.toml");

    /// A time on Tuesday 3 November 2026, in UTC (Paris is one hour ahead).
    fn at(time: &str) -> Timestamp {
        format!("2026-11-03T{time}Z").parse().unwrap()
    }

    fn siri(visits: Vec<Value>) -> Value {
        json!({ "Siri": { "ServiceDelivery": {
            "ResponseTimestamp": "2026-11-03T06:49:58Z",
            "StopMonitoringDelivery": [{ "Status": "true", "MonitoredStopVisit": visits }]
        }}})
    }

    /// A train's call; `call` holds its times and statuses.
    fn train(
        journey: &str,
        recorded: &str,
        mission: &str,
        destination: &str,
        call: Value,
    ) -> Value {
        let mut call = call;
        call["DeparturePlatformName"] = json!({ "value": "2" });
        let place = match destination {
            "Gare Centrale" => 200,
            "Les Aubépines" => 100,
            _ => 50,
        };
        json!({
            "RecordedAtTime": at(recorded).to_string(),
            "MonitoredVehicleJourney": {
                "LineRef": { "value": "STIF:Line::C01740:" },
                "FramedVehicleJourneyRef": { "DataFrameRef": { "value": "any" }, "DatedVehicleJourneyRef": journey },
                "DestinationRef": { "value": format!("STIF:StopArea:SP:{place}:") },
                "DestinationName": [{ "value": destination }],
                "JourneyNote": [{ "value": mission }],
                "MonitoredCall": call
            }
        })
    }

    fn metro(journey: &str, expected: &str) -> Value {
        json!({
            "RecordedAtTime": at("06:49:30").to_string(),
            "MonitoredVehicleJourney": {
                "LineRef": { "value": "STIF:Line::C02000:" },
                "FramedVehicleJourneyRef": { "DataFrameRef": { "value": "any" }, "DatedVehicleJourneyRef": journey },
                "DestinationName": [{ "value": "Porte Nord" }],
                "JourneyNote": [],
                "MonitoredCall": {
                    "ExpectedArrivalTime": at(expected).to_string(),
                    "ExpectedDepartureTime": at(expected).to_string(),
                    "DepartureStatus": "onTime",
                    "ArrivalStatus": ""
                }
            }
        })
    }

    fn departs(aimed: &str, expected: &str, status: &str) -> Value {
        json!({
            "AimedDepartureTime": at(aimed).to_string(),
            "ExpectedDepartureTime": at(expected).to_string(),
            "DepartureStatus": status,
            "ArrivalStatus": status
        })
    }

    fn arrives(aimed: &str, expected: &str, status: &str) -> Value {
        json!({
            "AimedArrivalTime": at(aimed).to_string(),
            "ExpectedArrivalTime": at(expected).to_string(),
            "ArrivalStatus": status
        })
    }

    /// The board stop of line L in the morning: what to show, and what not to.
    fn morning_board() -> Value {
        siri(vec![
            // Listed twice: the older record is stale.
            train(
                "j1",
                "06:30:00",
                "PALO",
                "Gare Centrale",
                departs("06:52:00", "06:55:00", "delayed"),
            ),
            train(
                "j1",
                "06:48:00",
                "PALO",
                "Gare Centrale",
                departs("06:52:00", "06:52:00", "onTime"),
            ),
            train(
                "j2",
                "06:47:00",
                "PILO",
                "Gare Centrale",
                departs("07:07:00", "07:11:00", "delayed"),
            ),
            train(
                "j3",
                "06:46:00",
                "PALO",
                "Gare Centrale",
                departs("07:22:00", "07:22:00", "cancelled"),
            ),
            // Gone two minutes ago.
            train(
                "j0",
                "06:47:00",
                "PALO",
                "Gare Centrale",
                departs("06:48:00", "06:48:00", "onTime"),
            ),
            // The other way: it went through the alight stop before.
            train(
                "j4",
                "06:40:00",
                "POLA",
                "Les Peupliers",
                departs("06:55:00", "06:55:00", "onTime"),
            ),
            // Ends here: nothing to board.
            train(
                "j5",
                "06:41:00",
                "POLA",
                "Les Aubépines",
                arrives("06:58:00", "06:58:00", "onTime"),
            ),
        ])
    }

    fn morning_alight() -> Value {
        siri(vec![
            train(
                "j1",
                "06:48:00",
                "PALO",
                "Gare Centrale",
                arrives("07:04:00", "07:04:00", "onTime"),
            ),
            // Listed twice, the stale record last.
            train(
                "j1",
                "06:20:00",
                "PALO",
                "Gare Centrale",
                arrives("07:04:00", "07:09:00", "delayed"),
            ),
            train(
                "j2",
                "06:47:00",
                "PILO",
                "Gare Centrale",
                arrives("07:19:00", "07:23:00", "delayed"),
            ),
            train(
                "j3",
                "06:46:00",
                "PALO",
                "Gare Centrale",
                arrives("07:34:00", "07:34:00", "cancelled"),
            ),
            train(
                "j4",
                "06:40:00",
                "POLA",
                "Les Peupliers",
                departs("06:40:00", "06:40:00", "onTime"),
            ),
        ])
    }

    fn morning_metro() -> Value {
        siri(vec![
            metro("m0", "06:48:00"),
            metro("m1", "06:53:30"),
            metro("m2", "06:58:10"),
        ])
    }

    fn stop(stop_point: &str, stop_area: &str, name: &str) -> RouteStop {
        RouteStop {
            stop_point: stop_point.into(),
            stop_area: stop_area.into(),
            name: name.into(),
        }
    }

    /// The example commute's routes: L between its two stations, both ways,
    /// and X with one platform per way.
    fn routes() -> BTreeMap<String, Vec<Route>> {
        let aubepines = stop(
            "stop_point:IDFM:monomodalStopPlace:100",
            "stop_area:IDFM:71100",
            "Les Aubépines",
        );
        let centrale = stop(
            "stop_point:IDFM:monomodalStopPlace:200",
            "stop_area:IDFM:71370",
            "Gare Centrale",
        );
        let peupliers = stop(
            "stop_point:IDFM:monomodalStopPlace:50",
            "stop_area:IDFM:71050",
            "Les Peupliers",
        );
        BTreeMap::from([
            (
                "C01740".to_string(),
                vec![
                    Route {
                        stops: vec![peupliers.clone(), aubepines.clone(), centrale.clone()],
                    },
                    Route {
                        stops: vec![centrale, aubepines, peupliers],
                    },
                ],
            ),
            (
                "C02000".to_string(),
                vec![
                    Route {
                        stops: vec![
                            stop(
                                "stop_point:IDFM:300",
                                "stop_area:IDFM:71380",
                                "Gare Centrale",
                            ),
                            stop("stop_point:IDFM:400", "stop_area:IDFM:71400", "Mairie"),
                        ],
                    },
                    Route {
                        stops: vec![
                            stop("stop_point:IDFM:401", "stop_area:IDFM:71400", "Mairie"),
                            stop(
                                "stop_point:IDFM:301",
                                "stop_area:IDFM:71380",
                                "Gare Centrale",
                            ),
                        ],
                    },
                ],
            ),
        ])
    }

    async fn answers(server: &MockServer, stop: &str, body: Value, times: u64) {
        Mock::given(method("GET"))
            .and(path("/stop-monitoring"))
            .and(query_param("MonitoringRef", stop))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(times)
            .mount(server)
            .await;
    }

    fn next_departures(server: &MockServer, config: &str) -> NextDepartures {
        NextDepartures::new(
            &Config::parse(config).unwrap(),
            Prim::new(&server.uri(), "test-key"),
            routes(),
        )
    }

    fn fixture(name: &str) -> Value {
        let path = format!("{}/../fixtures/api/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn the_morning_departures_match_the_fixture() {
        let server = MockServer::start().await;
        answers(&server, "STIF:StopArea:SP:100:", morning_board(), 1).await;
        answers(&server, "STIF:StopArea:SP:200:", morning_alight(), 1).await;
        answers(&server, "STIF:StopPoint:Q:300:", morning_metro(), 1).await;
        let departures = next_departures(&server, EXAMPLE);
        let view = departures.view(at("06:50:00")).await;
        assert_eq!(serde_json::to_value(&view).unwrap(), fixture("departures"));
    }

    #[tokio::test]
    async fn a_leg_without_routes_or_departures_says_so() {
        let server = MockServer::start().await;
        // In the evening, line L boards at Gare Centrale: only a train ending there.
        answers(
            &server,
            "STIF:StopArea:SP:200:",
            siri(vec![train(
                "j9",
                "15:00:00",
                "POLA",
                "Gare Centrale",
                arrives("16:30:00", "16:30:00", "onTime"),
            )]),
            1,
        )
        .await;
        answers(&server, "STIF:StopArea:SP:100:", siri(vec![]), 1).await;
        let departures = next_departures(&server, &EXAMPLE.replace("color = \"#7B61C9\"", ""));
        let mut routes = routes();
        routes.remove("C02000");
        departures.set_routes(routes);
        let view = departures.view(at("16:20:00")).await;
        assert_eq!(
            serde_json::to_value(&view).unwrap(),
            fixture("departures-unavailable")
        );
    }

    #[tokio::test]
    async fn a_stop_that_cannot_be_read_leaves_its_leg_unknown() {
        let server = MockServer::start().await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let view = next_departures(&server, EXAMPLE).view(at("06:50:00")).await;
        assert_eq!(
            view.legs
                .iter()
                .map(|leg| leg.departures.clone())
                .collect::<Vec<_>>(),
            [None, None]
        );
        assert_eq!(view.legs[0].from.as_deref(), Some("Les Aubépines"));
    }

    #[tokio::test]
    async fn a_train_leg_needs_its_alight_stop_too() {
        let server = MockServer::start().await;
        answers(&server, "STIF:StopArea:SP:100:", morning_board(), 1).await;
        answers(&server, "STIF:StopPoint:Q:300:", morning_metro(), 1).await;
        // No answer for the alight stop: a 404.
        let departures = next_departures(&server, EXAMPLE);
        let view = departures.view(at("06:50:00")).await;
        assert_eq!(view.legs[0].departures, None);
        assert!(view.legs[1].departures.is_some());
        // Nor with an alight stop that PRIM cannot monitor.
        let mut routes = routes();
        routes.get_mut("C01740").unwrap()[0].stops[2].stop_point = "sp:elsewhere".into();
        departures.set_routes(routes);
        assert_eq!(
            departures.view(at("06:50:30")).await.legs[0].departures,
            None
        );
    }

    #[tokio::test]
    async fn a_stop_is_read_at_most_once_a_minute() {
        let server = MockServer::start().await;
        answers(&server, "STIF:StopArea:SP:100:", morning_board(), 2).await;
        answers(&server, "STIF:StopArea:SP:200:", morning_alight(), 2).await;
        answers(&server, "STIF:StopPoint:Q:300:", morning_metro(), 2).await;
        let departures = next_departures(&server, EXAMPLE);
        let first = departures.view(at("06:50:00")).await;
        assert_eq!(departures.view(at("06:50:59")).await, first);
        departures.view(at("06:51:00")).await;
        // The mocks check their counts when the server drops.
    }

    #[tokio::test]
    async fn stops_are_not_read_on_demand_when_the_quota_runs_low() {
        let server = MockServer::start().await;
        for (stop, body) in [
            ("STIF:StopArea:SP:100:", morning_board()),
            ("STIF:StopArea:SP:200:", morning_alight()),
            ("STIF:StopPoint:Q:300:", morning_metro()),
        ] {
            Mock::given(path("/stop-monitoring"))
                .and(query_param("MonitoringRef", stop))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(body)
                        .insert_header("x-ratelimit-remaining-day", "150"),
                )
                .expect(2)
                .mount(&server)
                .await;
        }
        let departures = next_departures(&server, EXAMPLE);
        // Judged once per request: the first answer does not stop the next stop's read.
        assert!(
            departures.view(at("06:50:00")).await.legs[0]
                .departures
                .is_some()
        );
        // Still served from the last answer while it is fresh, then nothing.
        assert!(
            departures.view(at("06:50:30")).await.legs[0]
                .departures
                .is_some()
        );
        let low = departures.view(at("06:52:00")).await;
        assert_eq!(low.legs[0].departures, None);
        assert_eq!(low.legs[1].departures, None);
        // The quota starts again at midnight UTC.
        let next_day: Timestamp = "2026-11-04T06:50:00Z".parse().unwrap();
        assert!(departures.view(next_day).await.legs[0].departures.is_some());
    }

    #[test]
    fn a_stop_point_gives_its_monitoring_ref() {
        assert_eq!(
            monitoring_ref("stop_point:IDFM:monomodalStopPlace:47000").as_deref(),
            Some("STIF:StopArea:SP:47000:")
        );
        assert_eq!(
            monitoring_ref("stop_point:IDFM:462000").as_deref(),
            Some("STIF:StopPoint:Q:462000:")
        );
        assert_eq!(monitoring_ref("stop_point:IDFM:Elsewhere:1"), None);
        assert_eq!(monitoring_ref("stop_area:IDFM:71370"), None);
        assert!(one_way("stop_point:IDFM:462000"));
        assert!(!one_way("stop_point:IDFM:monomodalStopPlace:47000"));
    }

    #[test]
    fn a_leg_boards_and_alights_on_a_route_that_goes_its_way() {
        let routes = routes();
        let leg = |board: &str, alight: &str| Leg {
            line: "C02000".into(),
            board: board.into(),
            alight: alight.into(),
        };
        let names = |stops: Option<(&RouteStop, &RouteStop)>| {
            stops.map(|(board, alight)| (board.stop_point.clone(), alight.stop_point.clone()))
        };
        let x = &routes["C02000"];
        assert_eq!(
            names(leg_stops(
                &leg("stop_area:IDFM:71380", "stop_area:IDFM:71400"),
                x
            )),
            Some(("stop_point:IDFM:300".into(), "stop_point:IDFM:400".into()))
        );
        assert_eq!(
            names(leg_stops(
                &leg("stop_area:IDFM:71400", "stop_area:IDFM:71380"),
                x
            )),
            Some(("stop_point:IDFM:401".into(), "stop_point:IDFM:301".into()))
        );
        assert_eq!(
            names(leg_stops(
                &leg("stop_area:IDFM:71400", "stop_area:IDFM:99999"),
                x
            )),
            None
        );
        assert_eq!(
            names(leg_stops(
                &leg("stop_area:IDFM:99999", "stop_area:IDFM:71400"),
                x
            )),
            None
        );
    }

    #[test]
    fn answers_are_read_with_their_gaps() {
        let body = siri(vec![
            json!({
                "RecordedAtTime": "2026-11-03T06:00:00.000Z",
                "MonitoredVehicleJourney": {
                    "LineRef": { "value": "STIF:Line::C01740:" },
                    "FramedVehicleJourneyRef": { "DatedVehicleJourneyRef": "j7" },
                    "DestinationName": [],
                    "MonitoredCall": {
                        "AimedDepartureTime": "2026-11-03T07:00:00.000Z",
                        "DeparturePlatformName": { "value": "unknown" },
                        "DepartureStatus": "noReport"
                    }
                }
            }),
            // No departure status: the arrival's says it.
            train(
                "j8",
                "06:00:00",
                "PALO",
                "Gare Centrale",
                arrives("07:30:00", "07:30:00", "cancelled"),
            ),
            train(
                "j9",
                "06:00:00",
                "PALO",
                "Gare Centrale",
                json!({ "AimedDepartureTime": at("07:40:00").to_string(), "DepartureStatus": "", "ArrivalStatus": "delayed" }),
            ),
        ]);
        let visits = parse(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(
            visits[0],
            Visit {
                journey: "j7".into(),
                line: "C01740".into(),
                recorded_at: at("06:00:00"),
                mission: None,
                destination: String::new(),
                destination_ref: None,
                platform: None,
                aimed_departure: Some(at("07:00:00")),
                expected_departure: None,
                aimed_arrival: None,
                expected_arrival: None,
                status: CallStatus::OnTime,
            }
        );
        assert_eq!(visits[1].status, CallStatus::Cancelled);
        assert_eq!(visits[2].status, CallStatus::Delayed);
        // An empty stop has no visit at all.
        let empty = json!({ "Siri": { "ServiceDelivery": { "StopMonitoringDelivery": [{ "Status": "true" }] } } });
        assert_eq!(parse(&serde_json::to_vec(&empty).unwrap()).unwrap(), []);
        assert!(parse(b"<html>").is_err());
    }

    #[test]
    fn a_departure_without_an_expected_time_keeps_its_aimed_one() {
        let visits = parse(
            &serde_json::to_vec(&siri(vec![train(
                "j7",
                "06:00:00",
                "PALO",
                "Gare Centrale",
                json!({ "AimedDepartureTime": at("07:00:00").to_string(), "DepartureStatus": "onTime" }),
            )]))
            .unwrap(),
        )
        .unwrap();
        let shown = select("C01740", &visits, None, at("06:50:00"));
        assert_eq!(shown[0].expected, at("07:00:00"));
        assert_eq!(shown[0].aimed, Some(at("07:00:00")));
    }

    #[test]
    fn a_train_ending_at_the_alight_stop_stays_when_that_stop_drops_it() {
        // During an incident, a terminus drops trains from its arrivals long before they come.
        let board = siri(vec![
            train(
                "j1",
                "06:48:00",
                "PALO",
                "Gare Centrale",
                departs("06:52:00", "06:52:00", "onTime"),
            ),
            train(
                "j4",
                "06:40:00",
                "POLA",
                "Les Peupliers",
                departs("06:55:00", "06:55:00", "onTime"),
            ),
        ]);
        let board = parse(&serde_json::to_vec(&board).unwrap()).unwrap();
        let shown = select(
            "C01740",
            &board,
            Some(("STIF:StopArea:SP:200:", &[])),
            at("06:50:00"),
        );
        assert_eq!(
            shown
                .iter()
                .map(|d| (d.mission.as_deref(), d.arrival))
                .collect::<Vec<_>>(),
            [(Some("PALO"), None)]
        );
    }

    #[test]
    fn only_the_first_departures_are_shown() {
        let visits: Vec<Value> = (0..8)
            .map(|n| metro(&format!("m{n}"), &format!("07:0{n}:00")))
            .collect();
        let visits = parse(&serde_json::to_vec(&siri(visits)).unwrap()).unwrap();
        let shown = select("C02000", &visits, None, at("07:00:00"));
        assert_eq!(shown.len(), SHOWN);
        assert_eq!(shown[0].expected, at("07:00:00"));
        // Another line's trains are not this leg's.
        assert_eq!(select("C01740", &visits, None, at("07:00:00")), []);
    }

    #[test]
    fn the_morning_way_shows_until_its_window_ends() {
        let windows = Config::parse(EXAMPLE).unwrap().windows;
        assert_eq!(shown_direction(time(0, 0, 0, 0), &windows), "outbound");
        assert_eq!(shown_direction(time(8, 14, 59, 0), &windows), "outbound");
        assert_eq!(shown_direction(time(8, 15, 0, 0), &windows), "inbound");
        assert_eq!(shown_direction(time(23, 59, 0, 0), &windows), "inbound");
    }
}
