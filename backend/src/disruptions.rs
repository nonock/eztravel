//! Disruptions published by PRIM (`disruptions_bulk`), reduced to the lines we watch.
//!
//! The feed lists every disruption of the network, then, line by line, the
//! objects each disruption touches. Times are local to the network (Paris),
//! without an offset.

use std::collections::HashMap;

use jiff::civil::DateTime;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disruption {
    pub id: String,
    /// Watched lines the feed attaches the disruption to, as line codes ("C01740").
    pub lines: Vec<String>,
    /// Stops of watched lines the feed attaches the disruption to.
    pub stops: Vec<Place>,
    pub periods: Vec<Period>,
    pub updated: DateTime,
    pub cause: Cause,
    pub severity: Severity,
    pub title: String,
    pub short_message: Option<String>,
    /// HTML, as published.
    pub message: String,
    pub sections: Vec<Section>,
}

/// From `begin` included to `end` excluded, in network local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Period {
    #[serde(deserialize_with = "local_time")]
    pub begin: DateTime,
    #[serde(deserialize_with = "local_time")]
    pub end: DateTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub line: String,
    pub from: Place,
    pub to: Place,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Cause {
    #[serde(rename = "TRAVAUX")]
    Works,
    #[serde(rename = "PERTURBATION")]
    Incident,
    #[serde(rename = "INFORMATION")]
    Information,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    #[serde(rename = "BLOQUANTE")]
    Blocking,
    #[serde(rename = "PERTURBEE")]
    Disrupted,
    #[serde(rename = "INFORMATION")]
    Information,
    #[serde(other)]
    Unknown,
}

impl Disruption {
    /// The feed keeps a disruption listed after it ends: only its periods say whether it is current.
    pub fn is_active(&self, now: DateTime) -> bool {
        self.periods
            .iter()
            .any(|period| period.begin <= now && now < period.end)
    }
}

/// Disruptions attached to one of `lines` (line codes), in feed order.
pub fn parse(json: &[u8], lines: &[&str]) -> Result<Vec<Disruption>, serde_json::Error> {
    let feed: Feed = serde_json::from_slice(json)?;
    let mut attached: HashMap<String, (Vec<String>, Vec<Place>)> = HashMap::new();
    for line in feed.lines {
        let code = line_code(&line.id);
        if !lines.contains(&code) {
            continue;
        }
        for object in line.impacted_objects {
            for id in object.disruption_ids {
                let (codes, stops) = attached.entry(id).or_default();
                if !codes.iter().any(|known| known == code) {
                    codes.push(code.to_string());
                }
                let place = Place {
                    id: object.id.clone(),
                    name: object.name.clone(),
                };
                let is_stop = matches!(object.kind.as_str(), "stop_point" | "stop_area");
                if is_stop && !stops.contains(&place) {
                    stops.push(place);
                }
            }
        }
    }
    Ok(feed
        .disruptions
        .into_iter()
        .filter_map(|raw| {
            let (lines, stops) = attached.remove(&raw.id)?;
            Some(Disruption {
                id: raw.id,
                lines,
                stops,
                periods: raw.application_periods,
                updated: raw.last_update,
                cause: raw.cause,
                severity: raw.severity,
                title: raw.title,
                short_message: raw.short_message,
                message: raw.message,
                sections: raw
                    .impacted_sections
                    .unwrap_or_default()
                    .into_iter()
                    .map(|section| Section {
                        line: line_code(&section.line_id).to_string(),
                        from: section.from,
                        to: section.to,
                    })
                    .collect(),
            })
        })
        .collect())
}

/// "line:IDFM:C01740" -> "C01740".
fn line_code(id: &str) -> &str {
    id.rsplit(':').next().unwrap_or(id)
}

fn local_time<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DateTime, D::Error> {
    let text = String::deserialize(deserializer)?;
    DateTime::strptime("%Y%m%dT%H%M%S", &text).map_err(serde::de::Error::custom)
}

#[derive(Deserialize)]
struct Feed {
    disruptions: Vec<RawDisruption>,
    lines: Vec<RawLine>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDisruption {
    id: String,
    application_periods: Vec<Period>,
    #[serde(deserialize_with = "local_time")]
    last_update: DateTime,
    cause: Cause,
    severity: Severity,
    title: String,
    message: String,
    short_message: Option<String>,
    // Absent or null when the disruption has no section.
    #[serde(default)]
    impacted_sections: Option<Vec<RawSection>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSection {
    line_id: String,
    from: Place,
    to: Place,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLine {
    id: String,
    #[serde(default)]
    impacted_objects: Vec<RawObject>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawObject {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    name: String,
    disruption_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    const WATCHED: &str = "C01740";
    const ALSO_WATCHED: &str = "C02000";

    fn feed() -> String {
        serde_json::json!({
            "lastUpdatedDate": "2026-11-03T06:02:15.628Z",
            "disruptions": [
                {
                    "id": "d-incident",
                    "applicationPeriods": [{ "begin": "20261103T064433", "end": "20261103T083000" }],
                    "lastUpdate": "20261103T070243",
                    "cause": "PERTURBATION",
                    "severity": "PERTURBEE",
                    "title": "Ligne L : perturbations",
                    "message": "<p>Le trafic est perturbé entre Nanterre et Cergy. Motif : panne d'un train.</p>",
                    "shortMessage": "Trafic fortement perturbé",
                    "impactedSections": [{
                        "lineId": "line:IDFM:C01740",
                        "from": { "type": "stop_area", "id": "stop_area:IDFM:71100", "name": "Nanterre" },
                        "to": { "type": "stop_area", "id": "stop_area:IDFM:71200", "name": "Cergy" }
                    }]
                },
                {
                    "id": "d-elsewhere",
                    "applicationPeriods": [{ "begin": "20261103T000000", "end": "20261104T000000" }],
                    "lastUpdate": "20261102T120000",
                    "cause": "TRAVAUX",
                    "severity": "BLOQUANTE",
                    "title": "Bus 999 : travaux",
                    "message": "<p>Arrêt déplacé.</p>"
                },
                {
                    "id": "d-station",
                    "applicationPeriods": [{ "begin": "20261103T090000", "end": "20261104T043000" }],
                    "lastUpdate": "20261102T141022",
                    "cause": "TRAVAUX",
                    "severity": "BLOQUANTE",
                    "title": "Gare fermée",
                    "message": "<p>La gare est fermée.</p>",
                    "tags": ["Ascenseur"]
                },
                {
                    "id": "d-shared",
                    "applicationPeriods": [
                        { "begin": "20261103T060000", "end": "20261103T090000" },
                        { "begin": "20261103T160000", "end": "20261103T190000" }
                    ],
                    "lastUpdate": "20261102T080000",
                    "cause": "INFORMATION",
                    "severity": "INFORMATION",
                    "title": "Grève",
                    "message": "<p>Trafic réduit.</p>"
                }
            ],
            "lines": [
                {
                    "id": "line:IDFM:C01740", "name": "L", "shortName": "L", "mode": "LocalTrain",
                    "impactedObjects": [
                        { "type": "line", "id": "line:IDFM:C01740", "name": "L", "disruptionIds": ["d-incident", "d-shared", "unknown-id"] },
                        { "type": "stop_point", "id": "stop_point:IDFM:400", "name": "Gare A", "disruptionIds": ["d-station"] }
                    ]
                },
                {
                    "id": "line:IDFM:C09999", "name": "999", "shortName": "999", "mode": "Bus",
                    "impactedObjects": [
                        { "type": "line", "id": "line:IDFM:C09999", "name": "999", "disruptionIds": ["d-elsewhere"] }
                    ]
                },
                {
                    "id": "line:IDFM:C02000", "name": "X", "shortName": "X", "mode": "Metro",
                    "impactedObjects": [
                        { "type": "line", "id": "line:IDFM:C02000", "name": "X", "disruptionIds": ["d-shared"] }
                    ]
                }
            ]
        })
        .to_string()
    }

    fn parsed() -> Vec<Disruption> {
        parse(feed().as_bytes(), &[WATCHED, ALSO_WATCHED]).unwrap()
    }

    fn by_id<'a>(disruptions: &'a [Disruption], id: &str) -> &'a Disruption {
        disruptions.iter().find(|d| d.id == id).unwrap()
    }

    #[test]
    fn keeps_only_disruptions_of_watched_lines_in_feed_order() {
        let ids: Vec<_> = parsed().into_iter().map(|d| d.id).collect();
        assert_eq!(ids, ["d-incident", "d-station", "d-shared"]);
    }

    #[test]
    fn nothing_when_no_watched_line_is_disrupted() {
        assert!(parse(feed().as_bytes(), &["C05555"]).unwrap().is_empty());
    }

    #[test]
    fn a_disruption_on_several_watched_lines_appears_once() {
        let disruptions = parsed();
        assert_eq!(
            by_id(&disruptions, "d-shared").lines,
            [WATCHED, ALSO_WATCHED]
        );
        assert_eq!(by_id(&disruptions, "d-incident").lines, [WATCHED]);
    }

    #[test]
    fn records_the_stops_a_disruption_is_attached_to() {
        let disruptions = parsed();
        let station = Place {
            id: "stop_point:IDFM:400".into(),
            name: "Gare A".into(),
        };
        assert_eq!(by_id(&disruptions, "d-station").stops, [station]);
        assert_eq!(by_id(&disruptions, "d-station").lines, [WATCHED]);
        assert!(by_id(&disruptions, "d-incident").stops.is_empty());
    }

    #[test]
    fn a_disruption_on_a_line_and_its_stops_lists_the_line_once() {
        let json = feed().replace(r#"["d-station"]"#, r#"["d-station", "d-incident"]"#);
        let disruptions = parse(json.as_bytes(), &[WATCHED]).unwrap();
        let incident = by_id(&disruptions, "d-incident");
        assert_eq!(incident.lines, [WATCHED]);
        assert_eq!(
            incident.stops,
            [Place {
                id: "stop_point:IDFM:400".into(),
                name: "Gare A".into()
            }]
        );
    }

    #[test]
    fn only_stops_count_as_stops() {
        // Seen in the real feed: a line can list its whole network among the impacted objects.
        let network = r#"{ "type": "network", "id": "network:IDFM:1046", "name": "Transilien", "disruptionIds": ["d-shared"] },"#;
        let json = feed().replacen(
            r#"{"disruptionIds":["d-station"]"#,
            &format!("{network}{{\"disruptionIds\":[\"d-station\"]"),
            1,
        );
        assert!(json.contains("network:IDFM:1046"));
        let disruptions = parse(json.as_bytes(), &[WATCHED]).unwrap();
        let shared = by_id(&disruptions, "d-shared");
        assert_eq!(shared.lines, [WATCHED]);
        assert!(shared.stops.is_empty());
    }

    #[test]
    fn reads_every_field() {
        let disruptions = parsed();
        let incident = by_id(&disruptions, "d-incident");
        assert_eq!(
            incident.periods,
            [Period {
                begin: date(2026, 11, 3).at(6, 44, 33, 0),
                end: date(2026, 11, 3).at(8, 30, 0, 0)
            }]
        );
        assert_eq!(incident.updated, date(2026, 11, 3).at(7, 2, 43, 0));
        assert_eq!(incident.cause, Cause::Incident);
        assert_eq!(incident.severity, Severity::Disrupted);
        assert_eq!(incident.title, "Ligne L : perturbations");
        assert_eq!(
            incident.short_message.as_deref(),
            Some("Trafic fortement perturbé")
        );
        assert!(incident.message.contains("Motif : panne d'un train."));
        assert_eq!(
            incident.sections,
            [Section {
                line: "C01740".into(),
                from: Place {
                    id: "stop_area:IDFM:71100".into(),
                    name: "Nanterre".into()
                },
                to: Place {
                    id: "stop_area:IDFM:71200".into(),
                    name: "Cergy".into()
                },
            }]
        );
    }

    #[test]
    fn optional_fields_may_be_missing() {
        let disruptions = parsed();
        let station = by_id(&disruptions, "d-station");
        assert_eq!(station.short_message, None);
        assert!(station.sections.is_empty());
        assert_eq!(
            (station.cause, station.severity),
            (Cause::Works, Severity::Blocking)
        );
        let shared = by_id(&disruptions, "d-shared");
        assert_eq!(
            (shared.cause, shared.severity),
            (Cause::Information, Severity::Information)
        );
    }

    #[test]
    fn unknown_cause_or_severity_is_kept_as_unknown() {
        let json = feed()
            .replace("\"PERTURBATION\"", "\"NOUVELLE_CAUSE\"")
            .replace("\"PERTURBEE\"", "\"AUTRE\"");
        let disruptions = parse(json.as_bytes(), &[WATCHED]).unwrap();
        let incident = by_id(&disruptions, "d-incident");
        assert_eq!(
            (incident.cause, incident.severity),
            (Cause::Unknown, Severity::Unknown)
        );
    }

    #[test]
    fn rejects_a_malformed_feed() {
        assert!(parse(b"{\"disruptions\": [", &[WATCHED]).is_err());
        let bad_date = feed().replace("20261103T064433", "2026-11-03 06:44");
        assert!(parse(bad_date.as_bytes(), &[WATCHED]).is_err());
        let not_text = feed().replace(r#""20261103T064433""#, "20261103");
        assert!(parse(not_text.as_bytes(), &[WATCHED]).is_err());
    }

    #[test]
    fn active_from_begin_included_to_end_excluded() {
        let disruptions = parsed();
        let incident = by_id(&disruptions, "d-incident");
        assert!(!incident.is_active(date(2026, 11, 3).at(6, 44, 32, 0)));
        assert!(incident.is_active(date(2026, 11, 3).at(6, 44, 33, 0)));
        assert!(incident.is_active(date(2026, 11, 3).at(8, 29, 59, 0)));
        assert!(!incident.is_active(date(2026, 11, 3).at(8, 30, 0, 0)));
    }

    #[test]
    fn inactive_between_two_periods() {
        let disruptions = parsed();
        let shared = by_id(&disruptions, "d-shared");
        assert!(shared.is_active(date(2026, 11, 3).at(7, 0, 0, 0)));
        assert!(!shared.is_active(date(2026, 11, 3).at(12, 0, 0, 0)));
        assert!(shared.is_active(date(2026, 11, 3).at(18, 0, 0, 0)));
    }
}
