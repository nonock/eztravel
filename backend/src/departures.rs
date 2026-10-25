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

use crate::backups::{self, Backup};
use crate::calendar::{Calendar, Direction, WatchWindows};
use crate::config::Config;
use crate::leaving::{Refusal, check_pick, reminded_leg};
use crate::line_alerts::LineAlerts;
use crate::prim::{Prim, PrimError};
use crate::profile::{Pick, UsualTrains};
use crate::push::{self, LineLabel};
use crate::relevance::{Coord, Leg, Route, RouteStop};
use crate::watch::LineStatus;

/// Departures shown per leg, after those too soon to leave for on a ride with its time to the platform.
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
pub(crate) fn one_way(stop_point: &str) -> bool {
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
    /// The train or metro, the same at every stop of its run.
    pub journey: String,
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
    /// Picked in the app for the reminder to leave.
    pub picked: bool,
    /// The way's usual train, when none is picked today: the first from its time.
    pub usual: bool,
}

/// The next departures of `line` from the board stop, soonest first. With the
/// alight stop and its visits, only those that call there afterwards, or end there:
/// during an incident, a terminus drops trains from its arrivals long before they come.
/// With `reach`, the minutes to the platform, those too soon to leave for come
/// first, for whoever is already on the way, then [`SHOWN`] more.
pub fn select(
    line: &str,
    board: &[Visit],
    alight: Option<(&str, &[Visit])>,
    now: Timestamp,
    reach: Option<u32>,
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
                Some(alight) => serves(visit, expected, alight)?,
            };
            Some(Departure {
                journey: visit.journey.clone(),
                aimed: visit.aimed_departure,
                expected,
                arrival,
                mission: visit.mission.clone(),
                destination: visit.destination.clone(),
                platform: visit.platform.clone(),
                status: visit.status.name(),
                picked: false,
                usual: false,
            })
        })
        .collect();
    shown.sort_by_key(|departure| departure.expected);
    let too_soon = reach.map_or(0, |reach| {
        let reach = SignedDuration::from_mins(i64::from(reach));
        shown
            .iter()
            .take_while(|departure| departure.expected - reach < now)
            .count()
    });
    shown.truncate(too_soon + SHOWN);
    shown
}

/// Whether `visit`, leaving at `departure`, takes the leg to the alight stop
/// (its `MonitoringRef` and visits): it calls there later, or ends there.
/// Gives its arrival time there, when listed.
pub(crate) fn serves(
    visit: &Visit,
    departure: Timestamp,
    (stop, visits): (&str, &[Visit]),
) -> Option<Option<Timestamp>> {
    let arrival = visits
        .iter()
        .find(|other| other.journey == visit.journey)
        .and_then(Visit::reaches)
        .filter(|arrival| *arrival > departure);
    if arrival.is_none() && visit.destination_ref.as_deref() != Some(stop) {
        return None;
    }
    Some(arrival)
}

/// The way shown: the morning one until its window ends, the evening one after.
fn shown_direction(time: Time, windows: &WatchWindows) -> Direction {
    if time < windows.outbound.end {
        Direction::Outbound
    } else {
        Direction::Inbound
    }
}

/// The answer to `GET /departures`, in the shape fixed by `fixtures/api/`.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct View {
    pub direction: Direction,
    pub legs: Vec<LegView>,
    /// Those of the way's backups that avoid a line disrupted now.
    pub backups: Vec<BackupView>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct BackupView {
    /// The lines it stands in for, as disrupted on the way now.
    pub instead_of: Vec<LineStatus>,
    pub legs: Vec<BackupLeg>,
}

/// A ride of a backup: never picked, and maybe disrupted too.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct BackupLeg {
    #[serde(flatten)]
    pub leg: LegView,
    /// `blocking` or `disrupted`; none when it runs.
    pub severity: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct LegView {
    pub line: String,
    pub mode: &'static str,
    pub color: Option<String>,
    /// Stop names, unknown until the line's routes are.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Where `from` is, for the app to put the ride nearest the phone first.
    pub from_position: Option<Position>,
    /// Minutes from leaving to the platform, when configured: the app counts down from it.
    pub reach: Option<u32>,
    /// None when they cannot be read.
    pub departures: Option<Vec<Departure>>,
}

/// A place in decimal degrees.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Position {
    pub lat: serde_json::Number,
    pub lon: serde_json::Number,
}

impl Position {
    /// None when Navitia's figures are not numbers.
    fn of(coord: &Coord) -> Option<Self> {
        Some(Position {
            lat: coord.lat.parse().ok()?,
            lon: coord.lon.parse().ok()?,
        })
    }
}

/// Reads stops when the app asks, at most once a minute each.
pub struct NextDepartures {
    prim: Prim,
    outbound: Vec<Leg>,
    inbound: Vec<Leg>,
    backups: Vec<Backup>,
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

impl Cache {
    /// Whether stops may be read on demand: not when the day's quota runs low.
    fn may_read(&self, now: Timestamp) -> bool {
        let today = now.to_zoned(TimeZone::UTC).date();
        !self
            .quota
            .is_some_and(|(day, left)| day == today && left < RESERVE)
    }

    fn fresh(&self, stop: &str, now: Timestamp) -> Option<Vec<Visit>> {
        let (read_at, visits) = self.stops.get(stop)?;
        (now.duration_since(*read_at) < MAX_AGE).then(|| visits.clone())
    }
}

impl NextDepartures {
    pub fn new(config: &Config, prim: Prim, routes: BTreeMap<String, Vec<Route>>) -> Self {
        Self {
            prim,
            outbound: config.outbound.clone(),
            inbound: config.inbound.clone(),
            backups: config.backups.clone(),
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

    /// The way shown at `now`, and its legs.
    fn shown(&self, now: Timestamp) -> (Direction, &[Leg]) {
        let direction = shown_direction(now.to_zoned(self.time_zone.clone()).time(), &self.windows);
        (direction, self.legs(direction))
    }

    fn legs(&self, direction: Direction) -> &[Leg] {
        match direction {
            Direction::Outbound => &self.outbound,
            Direction::Inbound => &self.inbound,
        }
    }

    /// The board and alight stops of `leg`, once its line's routes are known.
    fn stops(&self, leg: &Leg) -> Option<(RouteStop, RouteStop)> {
        let routes = self
            .routes
            .lock()
            .expect("no thread panics while holding the routes");
        let (board, alight) = leg_stops(leg, routes.get(&leg.line)?)?;
        Some((board.clone(), alight.clone()))
    }

    /// `pick`, the train picked in the app, is marked when it is of this day and
    /// way; or else, on the ride reminded, the first train from the way's usual
    /// time. When the way has backups, the lines' disruptions, shared with the
    /// Alertes page, say which are needed.
    pub async fn view(
        &self,
        now: Timestamp,
        pick: Option<&Pick>,
        usual: &UsualTrains,
        feed: &LineAlerts,
    ) -> View {
        let (direction, legs) = self.shown(now);
        let local = now.to_zoned(self.time_zone.clone());
        let has_backups = self
            .backups
            .iter()
            .any(|backup| backup.direction == direction);
        let disruptions = if has_backups {
            feed.disruptions(now).await
        } else {
            None
        };
        let needed = disruptions.map_or(vec![], |disruptions| {
            let routes = self
                .routes
                .lock()
                .expect("no thread panics while holding the routes");
            backups::needed(
                &self.backups,
                direction,
                legs,
                &disruptions,
                local.datetime(),
                &routes,
            )
        });
        let today = local.date();
        let picked = pick
            .filter(|pick| pick.day == today && pick.direction == direction)
            .map(|pick| pick.journey.as_str());
        let reminded = reminded_leg(legs).map(|(leg, _)| leg);
        let usual_from = usual
            .of(direction)
            .filter(|_| picked.is_none())
            .map(|time| {
                today
                    .to_datetime(time)
                    .to_zoned(self.time_zone.clone())
                    .expect("a day's local time is within range")
                    .timestamp()
            });
        let mut cache = self.cache.lock().await;
        // Judged once per request, so that all of its legs are read or none.
        let may_read = cache.may_read(now);
        let mut views = vec![];
        for leg in legs {
            let mut view = self.leg_view(&mut cache, may_read, leg, now).await;
            let departures = &mut view.departures;
            for departure in departures.iter_mut().flatten() {
                departure.picked = Some(departure.journey.as_str()) == picked;
            }
            if let Some(from) =
                usual_from.filter(|_| reminded.is_some_and(|ride| std::ptr::eq(ride, leg)))
            {
                let usual = departures.iter_mut().flatten().find(|departure| {
                    departure.status != "cancelled" && departure.expected >= from
                });
                if let Some(usual) = usual {
                    usual.usual = true;
                }
            }
            views.push(view);
        }
        let mut backups = vec![];
        for backup in needed {
            let mut rides = vec![];
            for (ride, severity) in backup.backup.rides.iter().zip(backup.rides) {
                rides.push(BackupLeg {
                    leg: self.leg_view(&mut cache, may_read, ride, now).await,
                    severity: severity.map(push::severity_name),
                });
            }
            backups.push(BackupView {
                instead_of: backup
                    .avoided
                    .iter()
                    .map(|(line, severity)| {
                        let (name, mode, color) = push::label(line, &self.lines);
                        LineStatus {
                            line: name,
                            mode,
                            color,
                            severity: Some(push::severity_name(*severity)),
                        }
                    })
                    .collect(),
                legs: rides,
            });
        }
        View {
            direction,
            legs: views,
            backups,
        }
    }

    /// A ride with its next departures, none marked.
    async fn leg_view(
        &self,
        cache: &mut Cache,
        may_read: bool,
        leg: &Leg,
        now: Timestamp,
    ) -> LegView {
        let stops = self.stops(leg);
        let departures = match &stops {
            Some((board, alight)) => {
                self.departures(cache, may_read, leg, board, alight, now)
                    .await
            }
            None => None,
        };
        let (line, mode, color) = push::label(&leg.line, &self.lines);
        LegView {
            line,
            mode,
            color,
            from: stops.as_ref().map(|(board, _)| board.name.clone()),
            from_position: stops
                .as_ref()
                .and_then(|(board, _)| board.coord.as_ref())
                .and_then(Position::of),
            to: stops.map(|(_, alight)| alight.name),
            reach: leg.reach,
            departures,
        }
    }

    /// Whether a usual train leaving at `time` on the way `direction` can be
    /// reminded of: the way has a ride with its time to the platform, and leaving
    /// for the train falls in the way's window, when the poll loop gives reminders.
    pub fn check_usual(&self, direction: Direction, time: Time) -> Result<(), Refusal> {
        let window = self.windows.of(direction);
        let (_, reach) = reminded_leg(self.legs(direction)).ok_or(Refusal::Unknown)?;
        let leave = time
            .checked_sub(SignedDuration::from_mins(i64::from(reach)))
            .map_err(|_| Refusal::OutsideWindow)?;
        if leave < window.start || leave >= window.end {
            return Err(Refusal::OutsideWindow);
        }
        Ok(())
    }

    /// The train `journey`, picked at `now` for the reminder to leave: one of
    /// those shown on the ride the reminder is for, in the way shown, and one
    /// `check_pick` accepts.
    pub async fn pick(
        &self,
        journey: &str,
        now: Timestamp,
        calendar: &Calendar,
    ) -> Result<Pick, Refusal> {
        let (direction, legs) = self.shown(now);
        let (leg, reach) = reminded_leg(legs).ok_or(Refusal::Unknown)?;
        let (board, alight) = self.stops(leg).ok_or(Refusal::Unavailable)?;
        // A metro platform lists trains as they come, not one by one: no reminder there.
        if one_way(&board.stop_point) {
            return Err(Refusal::Unknown);
        }
        let mut cache = self.cache.lock().await;
        let may_read = cache.may_read(now);
        let departures = self
            .departures(&mut cache, may_read, leg, &board, &alight, now)
            .await
            .ok_or(Refusal::Unavailable)?;
        let departure = departures
            .iter()
            .find(|departure| departure.journey == journey && departure.status != "cancelled")
            .ok_or(Refusal::Unknown)?;
        let leave = departure.expected - SignedDuration::from_mins(i64::from(reach));
        check_pick(
            now,
            leave,
            direction,
            &self.time_zone,
            calendar,
            &self.windows,
        )?;
        Ok(Pick {
            day: leave.to_zoned(self.time_zone.clone()).date(),
            direction,
            journey: journey.to_string(),
            leave,
        })
    }

    async fn departures(
        &self,
        cache: &mut Cache,
        may_read: bool,
        leg: &Leg,
        board: &RouteStop,
        alight: &RouteStop,
        now: Timestamp,
    ) -> Option<Vec<Departure>> {
        let leaving = self
            .visits(cache, may_read, &monitoring_ref(&board.stop_point)?, now)
            .await?;
        if one_way(&board.stop_point) {
            return Some(select(&leg.line, &leaving, None, now, leg.reach));
        }
        let stop = monitoring_ref(&alight.stop_point)?;
        let arriving = self.visits(cache, may_read, &stop, now).await?;
        Some(select(
            &leg.line,
            &leaving,
            Some((&stop, &arriving)),
            now,
            leg.reach,
        ))
    }

    /// For the poll loop: a stop's visits, read whatever quota is left, since the
    /// windows come first. An answer under a minute old is used again.
    pub async fn read(&self, stop: &str, now: Timestamp) -> Result<Vec<Visit>, PrimError> {
        let mut cache = self.cache.lock().await;
        match cache.fresh(stop, now) {
            Some(visits) => Ok(visits),
            None => self.fetch(&mut cache, stop, now).await,
        }
    }

    /// A stop's visits, read again when older than a minute.
    async fn visits(
        &self,
        cache: &mut Cache,
        may_read: bool,
        stop: &str,
        now: Timestamp,
    ) -> Option<Vec<Visit>> {
        if let Some(visits) = cache.fresh(stop, now) {
            return Some(visits);
        }
        if !may_read {
            return None;
        }
        self.fetch(cache, stop, now)
            .await
            .inspect_err(|error| eprintln!("departures at {stop} unavailable: {error:?}"))
            .ok()
    }

    async fn fetch(
        &self,
        cache: &mut Cache,
        stop: &str,
        now: Timestamp,
    ) -> Result<Vec<Visit>, PrimError> {
        let answer = self.prim.stop_visits(stop).await?;
        if let Some(left) = answer.remaining {
            cache.quota = Some((now.to_zoned(TimeZone::UTC).date(), left));
        }
        cache
            .stops
            .insert(stop.to_string(), (now, answer.visits.clone()));
        Ok(answer.visits)
    }
}

/// The example commute's stops and answers, shared by the tests.
#[cfg(test)]
pub(crate) mod example {
    use super::*;
    use serde_json::{Value, json};

    /// A time on Tuesday 3 November 2026, in UTC (Paris is one hour ahead).
    pub(crate) fn at(time: &str) -> Timestamp {
        format!("2026-11-03T{time}Z").parse().unwrap()
    }

    pub(crate) fn siri(visits: Vec<Value>) -> Value {
        json!({ "Siri": { "ServiceDelivery": {
            "ResponseTimestamp": "2026-11-03T06:49:58Z",
            "StopMonitoringDelivery": [{ "Status": "true", "MonitoredStopVisit": visits }]
        }}})
    }

    /// A train's call; `call` holds its times and statuses.
    pub(crate) fn train(
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

    pub(crate) fn metro(journey: &str, expected: &str) -> Value {
        metro_on("C02000", journey, expected)
    }

    pub(crate) fn metro_on(line: &str, journey: &str, expected: &str) -> Value {
        json!({
            "RecordedAtTime": at("06:49:30").to_string(),
            "MonitoredVehicleJourney": {
                "LineRef": { "value": format!("STIF:Line::{line}:") },
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

    pub(crate) fn departs(aimed: &str, expected: &str, status: &str) -> Value {
        json!({
            "AimedDepartureTime": at(aimed).to_string(),
            "ExpectedDepartureTime": at(expected).to_string(),
            "DepartureStatus": status,
            "ArrivalStatus": status
        })
    }

    pub(crate) fn arrives(aimed: &str, expected: &str, status: &str) -> Value {
        json!({
            "AimedArrivalTime": at(aimed).to_string(),
            "ExpectedArrivalTime": at(expected).to_string(),
            "ArrivalStatus": status
        })
    }

    /// The board stop of line L in the morning: what to show, and what not to.
    pub(crate) fn morning_board() -> Value {
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

    pub(crate) fn morning_alight() -> Value {
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

    pub(crate) fn morning_metro() -> Value {
        siri(vec![
            metro("m0", "06:48:00"),
            metro("m1", "06:53:30"),
            metro("m2", "06:58:10"),
        ])
    }

    pub(crate) fn stop(stop_point: &str, stop_area: &str, name: &str) -> RouteStop {
        RouteStop {
            stop_point: stop_point.into(),
            stop_area: stop_area.into(),
            name: name.into(),
            coord: None,
        }
    }

    /// `stop` at a made-up place.
    fn placed(stop: RouteStop, lat: &str, lon: &str) -> RouteStop {
        RouteStop {
            coord: Some(Coord {
                lat: lat.into(),
                lon: lon.into(),
            }),
            ..stop
        }
    }

    /// The example commute's routes: L between its two stations, both ways,
    /// X with one platform per way, and Y, the backup of X, one way.
    pub(crate) fn routes() -> BTreeMap<String, Vec<Route>> {
        let aubepines = placed(
            stop(
                "stop_point:IDFM:monomodalStopPlace:100",
                "stop_area:IDFM:71100",
                "Les Aubépines",
            ),
            "48.947",
            "2.248",
        );
        let centrale = placed(
            stop(
                "stop_point:IDFM:monomodalStopPlace:200",
                "stop_area:IDFM:71370",
                "Gare Centrale",
            ),
            "48.876",
            "2.325",
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
                            placed(
                                stop(
                                    "stop_point:IDFM:300",
                                    "stop_area:IDFM:71380",
                                    "Gare Centrale",
                                ),
                                "48.875",
                                "2.326",
                            ),
                            stop("stop_point:IDFM:400", "stop_area:IDFM:71400", "Mairie"),
                        ],
                    },
                    Route {
                        stops: vec![
                            placed(
                                stop("stop_point:IDFM:401", "stop_area:IDFM:71400", "Mairie"),
                                "48.891",
                                "2.349",
                            ),
                            stop(
                                "stop_point:IDFM:301",
                                "stop_area:IDFM:71380",
                                "Gare Centrale",
                            ),
                        ],
                    },
                ],
            ),
            (
                "C03000".to_string(),
                vec![Route {
                    stops: vec![
                        placed(
                            stop(
                                "stop_point:IDFM:390",
                                "stop_area:IDFM:71390",
                                "Gare Centrale",
                            ),
                            "48.874",
                            "2.327",
                        ),
                        stop("stop_point:IDFM:410", "stop_area:IDFM:71400", "Mairie"),
                    ],
                }],
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::example::*;
    use super::*;
    use crate::disruptions::{Cause, Disruption, Period, Severity};
    use crate::profile::UsualTrains;
    use crate::testing::fixture;
    use jiff::civil::time;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const EXAMPLE: &str = include_str!("../config.example.toml");

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

    #[tokio::test]
    async fn the_morning_departures_match_the_fixture() {
        let server = MockServer::start().await;
        answers(&server, "STIF:StopArea:SP:100:", morning_board(), 1).await;
        answers(&server, "STIF:StopArea:SP:200:", morning_alight(), 1).await;
        answers(&server, "STIF:StopPoint:Q:300:", morning_metro(), 1).await;
        let departures = next_departures(&server, EXAMPLE);
        let usual = UsualTrains {
            outbound: Some(time(7, 50, 0, 0)),
            inbound: None,
        };
        let view = departures
            .view(at("06:50:00"), Some(&j2()), &usual, &quiet())
            .await;
        assert_eq!(serde_json::to_value(&view).unwrap(), fixture("departures"));
    }

    /// The 7:07 in Paris, 4 minutes late, picked this Tuesday morning: leave at 7:59.
    fn j2() -> Pick {
        Pick {
            day: jiff::civil::date(2026, 11, 3),
            direction: Direction::Outbound,
            journey: "j2".into(),
            leave: at("06:59:00"),
        }
    }

    /// Picks `journey` at 7:50 in Paris, the stations answering as in the morning fixture.
    async fn pick_at_ten_to_eight(
        config: &str,
        journey: &str,
        calendar: Option<&Calendar>,
    ) -> Result<Pick, Refusal> {
        let server = MockServer::start().await;
        Mock::given(path("/stop-monitoring"))
            .and(query_param("MonitoringRef", "STIF:StopArea:SP:100:"))
            .respond_with(ResponseTemplate::new(200).set_body_json(morning_board()))
            .mount(&server)
            .await;
        Mock::given(path("/stop-monitoring"))
            .and(query_param("MonitoringRef", "STIF:StopArea:SP:200:"))
            .respond_with(ResponseTemplate::new(200).set_body_json(morning_alight()))
            .mount(&server)
            .await;
        let parsed = Config::parse(config).unwrap();
        let calendar = calendar.unwrap_or(&parsed.calendar);
        next_departures(&server, config)
            .pick(journey, at("06:50:00"), calendar)
            .await
    }

    #[tokio::test]
    async fn a_train_shown_on_the_ride_reminded_is_picked_with_its_time_to_leave() {
        assert_eq!(pick_at_ten_to_eight(EXAMPLE, "j2", None).await, Ok(j2()));
        // Already too late to leave for: the 7:52, 12 minutes away.
        assert_eq!(
            pick_at_ten_to_eight(EXAMPLE, "j1", None).await,
            Err(Refusal::TooLate)
        );
        // Cancelled, or not shown, or on another ride.
        for journey in ["j3", "j9", "m1"] {
            assert_eq!(
                pick_at_ten_to_eight(EXAMPLE, journey, None).await,
                Err(Refusal::Unknown),
                "{journey}"
            );
        }
    }

    #[tokio::test]
    async fn at_rush_hour_the_ride_reminded_still_shows_five_trains_to_pick() {
        // A train every two minutes from 7:51 in Paris, twelve minutes from home.
        let rush = siri(
            (0..12)
                .map(|n| {
                    let time = format!(
                        "{}:00",
                        (at("06:51:00") + SignedDuration::from_mins(2 * n)).strftime("%H:%M")
                    );
                    train(
                        &format!("t{n}"),
                        "06:49:00",
                        "PALO",
                        "Gare Centrale",
                        departs(&time, &time, "onTime"),
                    )
                })
                .collect(),
        );
        let server = MockServer::start().await;
        answers(&server, "STIF:StopArea:SP:100:", rush, 1).await;
        answers(&server, "STIF:StopArea:SP:200:", siri(vec![]), 1).await;
        let departures = next_departures(&server, EXAMPLE);
        let view = departures
            .view(at("06:50:00"), None, &UsualTrains::default(), &quiet())
            .await;
        // Six leave before the platform can be reached: still shown, for whoever is on the way.
        let shown: Vec<_> = view.legs[0]
            .departures
            .iter()
            .flatten()
            .map(|departure| departure.journey.as_str())
            .collect();
        assert_eq!(
            shown,
            [
                "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8", "t9", "t10"
            ]
        );
        let calendar = Config::parse(EXAMPLE).unwrap().calendar;
        assert_eq!(
            departures
                .pick("t10", at("06:50:00"), &calendar)
                .await
                .map(|pick| pick.leave),
            Ok(at("06:59:00"))
        );
        assert_eq!(
            departures.pick("t11", at("06:50:00"), &calendar).await,
            Err(Refusal::Unknown)
        );
    }

    #[test]
    fn a_usual_train_is_one_whose_time_to_leave_falls_in_the_window() {
        let server_less = NextDepartures::new(
            &Config::parse(EXAMPLE).unwrap(),
            Prim::new("http://127.0.0.1:9", "key"),
            routes(),
        );
        let check =
            |direction, hour, minute| server_less.check_usual(direction, time(hour, minute, 0, 0));
        // Twelve minutes to the platform in the morning, whose window is 6:45 to 8:15.
        assert_eq!(check(Direction::Outbound, 6, 57), Ok(()));
        assert_eq!(check(Direction::Outbound, 8, 26), Ok(()));
        assert_eq!(
            check(Direction::Outbound, 6, 56),
            Err(Refusal::OutsideWindow)
        );
        assert_eq!(
            check(Direction::Outbound, 8, 27),
            Err(Refusal::OutsideWindow)
        );
        // Twenty-five in the evening, from 16:00 to 17:30; just after midnight, leaving is the day before.
        assert_eq!(check(Direction::Inbound, 16, 25), Ok(()));
        assert_eq!(
            check(Direction::Inbound, 16, 24),
            Err(Refusal::OutsideWindow)
        );
        assert_eq!(check(Direction::Inbound, 0, 5), Err(Refusal::OutsideWindow));
        // A way without a time to the platform has no reminder.
        let without = NextDepartures::new(
            &Config::parse(&EXAMPLE.replace("reach = 12", "")).unwrap(),
            Prim::new("http://127.0.0.1:9", "key"),
            routes(),
        );
        assert_eq!(
            without.check_usual(Direction::Outbound, time(7, 52, 0, 0)),
            Err(Refusal::Unknown)
        );
    }

    #[tokio::test]
    async fn the_usual_train_is_marked_unless_one_is_picked_today() {
        let server = MockServer::start().await;
        answers(&server, "STIF:StopArea:SP:100:", morning_board(), 1).await;
        answers(&server, "STIF:StopArea:SP:200:", morning_alight(), 1).await;
        answers(&server, "STIF:StopPoint:Q:300:", morning_metro(), 1).await;
        let departures = next_departures(&server, EXAMPLE);
        let usual = |hour, minute| UsualTrains {
            outbound: Some(time(hour, minute, 0, 0)),
            inbound: None,
        };
        let marked = |view: &View| -> Vec<String> {
            view.legs
                .iter()
                .flat_map(|leg| leg.departures.iter().flatten())
                .filter(|departure| departure.usual)
                .map(|departure| departure.journey.clone())
                .collect()
        };
        // The first train from 8:05 in Paris: the 8:07, leaving at 8:11.
        let view = departures
            .view(at("06:50:00"), None, &usual(8, 5), &quiet())
            .await;
        assert_eq!(marked(&view), ["j2"]);
        // From 8:12: the 8:22 is cancelled, and none other is listed.
        let view = departures
            .view(at("06:50:00"), None, &usual(8, 12), &quiet())
            .await;
        assert_eq!(marked(&view), Vec::<String>::new());
        // The train picked today is the one reminded.
        let view = departures
            .view(at("06:50:00"), Some(&j2()), &usual(7, 50), &quiet())
            .await;
        assert_eq!(marked(&view), Vec::<String>::new());
        // The other way's usual train is not this one's.
        let evening = UsualTrains {
            outbound: None,
            inbound: Some(time(7, 50, 0, 0)),
        };
        let view = departures
            .view(at("06:50:00"), None, &evening, &quiet())
            .await;
        assert_eq!(marked(&view), Vec::<String>::new());
    }

    /// The feed as the poll loop read it at 7:50 in Paris, PRIM unreachable since.
    fn feed(disruptions: &[Disruption]) -> LineAlerts {
        let alerts = unread();
        alerts.store(at("06:50:00"), disruptions);
        alerts
    }

    /// Nothing on the lines.
    fn quiet() -> LineAlerts {
        feed(&[])
    }

    /// Never read, and PRIM unreachable.
    fn unread() -> LineAlerts {
        LineAlerts::new(
            &Config::parse(EXAMPLE).unwrap(),
            Prim::new("http://127.0.0.1:9", "key"),
        )
    }

    /// An incident on `line` from 7:00 to 10:00 in Paris this Tuesday.
    fn incident(line: &str, severity: Severity) -> Disruption {
        let day = jiff::civil::date(2026, 11, 3);
        Disruption {
            id: format!("{line}-incident"),
            lines: vec![line.into()],
            stops: vec![],
            periods: vec![Period {
                begin: day.at(7, 0, 0, 0),
                end: day.at(10, 0, 0, 0),
            }],
            updated: day.at(7, 0, 0, 0),
            cause: Cause::Incident,
            severity,
            title: "Incident".into(),
            short_message: None,
            message: String::new(),
            sections: vec![],
        }
    }

    #[test]
    fn a_stop_placed_with_words_has_no_position() {
        let coord = |lat: &str| Coord {
            lat: lat.into(),
            lon: "2.325".into(),
        };
        assert!(Position::of(&coord("48.876")).is_some());
        assert_eq!(Position::of(&coord("north")), None);
    }

    #[tokio::test]
    async fn the_backup_departures_match_the_fixture() {
        let server = MockServer::start().await;
        answers(&server, "STIF:StopArea:SP:100:", morning_board(), 1).await;
        answers(&server, "STIF:StopArea:SP:200:", morning_alight(), 1).await;
        answers(&server, "STIF:StopPoint:Q:300:", morning_metro(), 1).await;
        let line_y = siri(vec![
            metro_on("C03000", "y1", "06:51:00"),
            metro_on("C03000", "y2", "06:57:00"),
        ]);
        answers(&server, "STIF:StopPoint:Q:390:", line_y, 1).await;
        let departures = next_departures(&server, EXAMPLE);
        let feed = feed(&[incident("C02000", Severity::Blocking)]);
        let view = departures
            .view(at("06:50:00"), None, &UsualTrains::default(), &feed)
            .await;
        assert_eq!(
            serde_json::to_value(&view).unwrap(),
            fixture("departures-backup")
        );
    }

    #[tokio::test]
    async fn a_backup_is_read_only_while_a_line_it_avoids_is_disrupted() {
        let server = MockServer::start().await;
        answers(&server, "STIF:StopArea:SP:100:", morning_board(), 1).await;
        answers(&server, "STIF:StopArea:SP:200:", morning_alight(), 1).await;
        answers(&server, "STIF:StopPoint:Q:300:", morning_metro(), 1).await;
        answers(&server, "STIF:StopPoint:Q:390:", siri(vec![]), 1).await;
        let departures = next_departures(&server, EXAMPLE);
        let usual = UsualTrains::default();
        // Without the feed, or with nothing on line X: no backup, and Y is not read.
        for feed in [unread(), feed(&[incident("C01740", Severity::Blocking)])] {
            let view = departures.view(at("06:50:00"), None, &usual, &feed).await;
            assert_eq!(view.backups, []);
        }
        // The backup's own line disrupted too: said on its ride.
        let feed = feed(&[
            incident("C02000", Severity::Unknown),
            incident("C03000", Severity::Disrupted),
        ]);
        let view = departures.view(at("06:50:30"), None, &usual, &feed).await;
        assert_eq!(view.backups.len(), 1);
        assert_eq!(view.backups[0].instead_of[0].severity, Some("disrupted"));
        assert_eq!(view.backups[0].legs[0].severity, Some("disrupted"));
        assert_eq!(view.backups[0].legs[0].leg.departures, Some(vec![]));
    }

    #[tokio::test]
    async fn only_a_way_with_backups_reads_the_disruptions() {
        let server = MockServer::start().await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(ResponseTemplate::new(200).set_body_json(siri(vec![])))
            .mount(&server)
            .await;
        // The example has a backup in the morning only.
        Mock::given(path("/disruptions_bulk/disruptions/v2"))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;
        let departures = next_departures(&server, EXAMPLE);
        let feed = LineAlerts::new(
            &Config::parse(EXAMPLE).unwrap(),
            Prim::new(&server.uri(), "key"),
        );
        let usual = UsualTrains::default();
        departures.view(at("16:20:00"), None, &usual, &feed).await;
        let view = departures.view(at("06:50:00"), None, &usual, &feed).await;
        assert_eq!(view.backups, []);
    }

    #[tokio::test]
    async fn a_pick_is_refused_when_it_would_never_be_reminded() {
        let config = Config::parse(EXAMPLE).unwrap();
        let away = Calendar {
            days: std::collections::BTreeMap::from([(
                jiff::civil::date(2026, 11, 3),
                crate::calendar::Day::Away,
            )]),
            ..config.calendar
        };
        assert_eq!(
            pick_at_ten_to_eight(EXAMPLE, "j2", Some(&away)).await,
            Err(Refusal::NotOfficeDay)
        );
        // Leaving at 7:59 when the window closes at 7:55.
        let early = EXAMPLE.replace(r#"end = "08:15""#, r#"end = "07:55""#);
        assert_eq!(
            pick_at_ten_to_eight(&early, "j2", None).await,
            Err(Refusal::OutsideWindow)
        );
        // The ride with its time to the platform is the metro's, which lists no train by its timetable.
        let metro = EXAMPLE
            .replace(
                "reach = 12
",
                "",
            )
            .replace(
                "alight = \"stop_area:IDFM:71400\"\n\n[[inbound]]",
                "alight = \"stop_area:IDFM:71400\"\nreach = 5\n\n[[inbound]]",
            );
        assert_eq!(
            pick_at_ten_to_eight(&metro, "m1", None).await,
            Err(Refusal::Unknown)
        );
    }

    #[tokio::test]
    async fn a_pick_cannot_be_judged_without_the_departures() {
        let server = MockServer::start().await;
        Mock::given(path("/stop-monitoring"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let config = Config::parse(EXAMPLE).unwrap();
        let departures = next_departures(&server, EXAMPLE);
        assert_eq!(
            departures
                .pick("j2", at("06:50:00"), &config.calendar)
                .await,
            Err(Refusal::Unavailable)
        );
        // Nor without the stops of the ride.
        departures.set_routes(BTreeMap::new());
        assert_eq!(
            departures
                .pick("j2", at("06:50:00"), &config.calendar)
                .await,
            Err(Refusal::Unavailable)
        );
        // A way without a ride to remind of.
        let none = EXAMPLE
            .replace(
                "reach = 12
",
                "",
            )
            .replace(
                "[leave]
outbound = \"07:30\"
",
                "",
            );
        let config = Config::parse(&none).unwrap();
        assert_eq!(
            next_departures(&server, &none)
                .pick("j2", at("06:50:00"), &config.calendar)
                .await,
            Err(Refusal::Unknown)
        );
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
        let view = departures
            .view(at("16:20:00"), None, &UsualTrains::default(), &quiet())
            .await;
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
        let view = next_departures(&server, EXAMPLE)
            .view(at("06:50:00"), None, &UsualTrains::default(), &quiet())
            .await;
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
        let view = departures
            .view(at("06:50:00"), None, &UsualTrains::default(), &quiet())
            .await;
        assert_eq!(view.legs[0].departures, None);
        assert!(view.legs[1].departures.is_some());
        // Nor with an alight stop that PRIM cannot monitor.
        let mut routes = routes();
        routes.get_mut("C01740").unwrap()[0].stops[2].stop_point = "sp:elsewhere".into();
        departures.set_routes(routes);
        assert_eq!(
            departures
                .view(at("06:50:30"), None, &UsualTrains::default(), &quiet())
                .await
                .legs[0]
                .departures,
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
        let first = departures
            .view(at("06:50:00"), None, &UsualTrains::default(), &quiet())
            .await;
        assert_eq!(
            departures
                .view(at("06:50:59"), None, &UsualTrains::default(), &quiet())
                .await,
            first
        );
        departures
            .view(at("06:51:00"), None, &UsualTrains::default(), &quiet())
            .await;
        // The mocks check their counts when the server drops.
    }

    #[tokio::test]
    async fn the_poll_loop_uses_what_the_app_just_read() {
        let server = MockServer::start().await;
        answers(&server, "STIF:StopArea:SP:100:", morning_board(), 2).await;
        answers(&server, "STIF:StopArea:SP:200:", morning_alight(), 1).await;
        answers(&server, "STIF:StopPoint:Q:300:", morning_metro(), 1).await;
        let departures = next_departures(&server, EXAMPLE);
        departures
            .view(at("06:50:00"), None, &UsualTrains::default(), &quiet())
            .await;
        let read = departures
            .read("STIF:StopArea:SP:100:", at("06:50:30"))
            .await;
        assert_eq!(read.unwrap().len(), 6);
        // Older than a minute: read again, whatever the quota says.
        departures
            .read("STIF:StopArea:SP:100:", at("06:51:00"))
            .await
            .unwrap();
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
            departures
                .view(at("06:50:00"), None, &UsualTrains::default(), &quiet())
                .await
                .legs[0]
                .departures
                .is_some()
        );
        // Still served from the last answer while it is fresh, then nothing.
        assert!(
            departures
                .view(at("06:50:30"), None, &UsualTrains::default(), &quiet())
                .await
                .legs[0]
                .departures
                .is_some()
        );
        let low = departures
            .view(at("06:52:00"), None, &UsualTrains::default(), &quiet())
            .await;
        assert_eq!(low.legs[0].departures, None);
        assert_eq!(low.legs[1].departures, None);
        // The quota starts again at midnight UTC.
        let next_day: Timestamp = "2026-11-04T06:50:00Z".parse().unwrap();
        assert!(
            departures
                .view(next_day, None, &UsualTrains::default(), &quiet())
                .await
                .legs[0]
                .departures
                .is_some()
        );
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
            reach: None,
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
        let shown = select("C01740", &visits, None, at("06:50:00"), None);
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
            None,
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
        let shown = select("C02000", &visits, None, at("07:00:00"), None);
        assert_eq!(shown.len(), SHOWN);
        assert_eq!(shown[0].expected, at("07:00:00"));
        // Another line's trains are not this leg's.
        assert_eq!(select("C01740", &visits, None, at("07:00:00"), None), []);
    }

    #[test]
    fn trains_too_soon_to_leave_for_are_shown_before_the_first_that_can_be_caught() {
        let visits: Vec<Value> = (0..9)
            .map(|n| metro(&format!("m{n}"), &format!("07:0{n}:00")))
            .collect();
        let visits = parse(&serde_json::to_vec(&siri(visits)).unwrap()).unwrap();
        // Three minutes to the platform: leaving now catches the 7:03. The 7:00 to
        // 7:02 are still shown, for whoever is already on the way.
        let shown = select("C02000", &visits, None, at("07:00:00"), Some(3));
        assert_eq!(shown.len(), 3 + SHOWN);
        assert_eq!(shown[0].expected, at("07:00:00"));
        assert_eq!(shown[3].expected, at("07:03:00"));
        assert_eq!(shown[3 + SHOWN - 1].expected, at("07:07:00"));
    }

    #[test]
    fn the_morning_way_shows_until_its_window_ends() {
        let windows = Config::parse(EXAMPLE).unwrap().windows;
        let shown = |time| shown_direction(time, &windows);
        assert_eq!(shown(time(0, 0, 0, 0)), Direction::Outbound);
        assert_eq!(shown(time(8, 14, 59, 0)), Direction::Outbound);
        assert_eq!(shown(time(8, 15, 0, 0)), Direction::Inbound);
        assert_eq!(shown(time(23, 59, 0, 0)), Direction::Inbound);
    }
}
