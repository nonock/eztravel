//! What was sent to the phone, kept so that a notification can be explained
//! after the fact: PRIM drops ended records within the hour, and the host
//! keeps its logs for hours only. Also kept: each window that closed on a
//! line still disrupted, since no "ended" follows then. Saved in the data
//! folder, the latest few hundred entries.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use jiff::Timestamp;
use jiff::civil::DateTime;
use serde::{Deserialize, Serialize};

use crate::alerts::Notification;
use crate::disruptions::{Disruption, Severity};
use crate::push::{self, LineLabel};
use crate::service::{load, save};

/// Weeks of busy mornings and evenings.
pub const MAX_ENTRIES: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub at: Timestamp,
    /// The notification's data as sent, or a window that closed (`type` `window_closed`).
    pub data: BTreeMap<String, String>,
    /// How many phones accepted it.
    #[serde(default)]
    pub delivered: usize,
    /// The PRIM records behind it.
    #[serde(default)]
    pub records: Vec<Record>,
}

/// A PRIM record as it was when it was sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub cause: String,
    pub severity: String,
    pub periods: Vec<(DateTime, DateTime)>,
    pub sections: Vec<SectionRecord>,
    pub title: String,
    pub text: String,
}

/// A disrupted section, in its direction: from one stop area to another.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionRecord {
    pub from: String,
    pub from_name: String,
    pub to: String,
    pub to_name: String,
}

impl From<&Disruption> for Record {
    fn from(disruption: &Disruption) -> Self {
        Record {
            id: disruption.id.clone(),
            cause: format!("{:?}", disruption.cause),
            severity: format!("{:?}", disruption.severity),
            periods: disruption
                .periods
                .iter()
                .map(|period| (period.begin, period.end))
                .collect(),
            sections: disruption
                .sections
                .iter()
                .map(|section| SectionRecord {
                    from: section.from.id.clone(),
                    from_name: section.from.name.clone(),
                    to: section.to.id.clone(),
                    to_name: section.to.name.clone(),
                })
                .collect(),
            title: disruption.title.clone(),
            text: push::plain_text(&disruption.message),
        }
    }
}

/// The records behind `notification`.
pub fn records(notification: &Notification) -> Vec<Record> {
    match notification {
        Notification::Started { disruptions, .. }
        | Notification::Changed { disruptions, .. }
        | Notification::Planned { disruptions }
        | Notification::Tomorrow { disruptions, .. } => {
            disruptions.iter().map(Record::from).collect()
        }
        Notification::Ended { .. }
        | Notification::Outage
        | Notification::Recovered
        | Notification::TrainCancelled { .. }
        | Notification::TrainBack { .. }
        | Notification::Leave { .. }
        | Notification::Late { .. } => vec![],
    }
}

/// A window closed while `line` was still disrupted.
pub fn window_closed(
    line: &str,
    severity: Severity,
    labels: &[LineLabel],
) -> BTreeMap<String, String> {
    let (name, mode, _) = push::label(line, labels);
    BTreeMap::from([
        ("type".to_string(), "window_closed".to_string()),
        ("line".to_string(), name),
        ("mode".to_string(), mode.to_string()),
        (
            "severity".to_string(),
            push::severity_name(severity).to_string(),
        ),
    ])
}

/// The entries behind a lock, oldest first, saved after each one.
pub struct History {
    path: PathBuf,
    entries: Mutex<Vec<Entry>>,
}

impl History {
    /// Picks up the saved entries; an unreadable file starts afresh.
    pub fn open(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            entries: Mutex::new(load(path).unwrap_or_default()),
        }
    }

    pub fn record(&self, entry: Entry) {
        let mut entries = self
            .entries
            .lock()
            .expect("no thread panics while holding the history");
        entries.push(entry);
        let extra = entries.len().saturating_sub(MAX_ENTRIES);
        entries.drain(..extra);
        save(&self.path, &*entries);
    }

    /// The `limit` latest entries, newest first.
    pub fn latest(&self, limit: usize) -> Vec<Entry> {
        self.entries
            .lock()
            .expect("no thread panics while holding the history")
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disruptions::{Cause, Period, Place, Section};
    use crate::push::Mode;
    use jiff::civil::date;

    fn at(minute: i8) -> Timestamp {
        date(2026, 11, 3)
            .at(6, minute, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
            .timestamp()
    }

    fn entry(minute: i8, kind: &str) -> Entry {
        Entry {
            at: at(minute),
            data: BTreeMap::from([("type".to_string(), kind.to_string())]),
            delivered: 1,
            records: vec![],
        }
    }

    fn kinds(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.data["type"].as_str())
            .collect()
    }

    #[test]
    fn entries_come_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let history = History::open(&dir.path().join("history.json"));
        history.record(entry(0, "planned"));
        history.record(entry(1, "started"));
        history.record(entry(2, "ended"));
        assert_eq!(kinds(&history.latest(10)), ["ended", "started", "planned"]);
        assert_eq!(kinds(&history.latest(2)), ["ended", "started"]);
    }

    #[test]
    fn only_the_latest_entries_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let history = History::open(&dir.path().join("history.json"));
        for _ in 0..MAX_ENTRIES {
            history.record(entry(0, "old"));
        }
        history.record(entry(1, "new"));
        let all = history.latest(usize::MAX);
        assert_eq!(all.len(), MAX_ENTRIES);
        assert_eq!(all[0].data["type"], "new");
    }

    #[test]
    fn the_history_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        History::open(&path).record(entry(0, "started"));
        assert_eq!(History::open(&path).latest(10), [entry(0, "started")]);
        // Unreadable, it starts afresh rather than stopping the service.
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(History::open(&path).latest(10), []);
    }

    const BEGIN: jiff::civil::DateTime = date(2026, 11, 3).at(7, 0, 0, 0);
    const END: jiff::civil::DateTime = date(2026, 11, 3).at(9, 0, 0, 0);

    /// A one-way disruption of line L, from Nanterre to Cergy.
    fn one_way() -> Disruption {
        let (begin, end) = (BEGIN, END);
        let place = |id: &str, name: &str| Place {
            id: id.into(),
            name: name.into(),
        };
        Disruption {
            id: "d1".into(),
            lines: vec!["C01740".into()],
            stops: vec![],
            periods: vec![Period { begin, end }],
            updated: begin,
            cause: Cause::Incident,
            severity: Severity::Disrupted,
            title: "Ligne L : perturbations".into(),
            short_message: None,
            message: "<p>Le trafic est perturbé de Nanterre vers Cergy.</p>".into(),
            sections: vec![Section {
                line: "C01740".into(),
                from: place("sa:1", "Nanterre"),
                to: place("sa:2", "Cergy"),
            }],
        }
    }

    #[test]
    fn a_record_keeps_what_explains_a_notification_after_prim_forgets_it() {
        let (begin, end) = (BEGIN, END);
        assert_eq!(
            Record::from(&one_way()),
            Record {
                id: "d1".into(),
                cause: "Incident".into(),
                severity: "Disrupted".into(),
                periods: vec![(begin, end)],
                // In order: the direction is what tells a one-way disruption.
                sections: vec![SectionRecord {
                    from: "sa:1".into(),
                    from_name: "Nanterre".into(),
                    to: "sa:2".into(),
                    to_name: "Cergy".into(),
                }],
                title: "Ligne L : perturbations".into(),
                text: "Le trafic est perturbé de Nanterre vers Cergy.".into(),
            }
        );
    }

    #[test]
    fn the_records_are_those_behind_the_notification() {
        let changed = Notification::Changed {
            line: "C01740".into(),
            from: Severity::Blocking,
            to: Severity::Disrupted,
            disruptions: vec![one_way()],
            scope: crate::relevance::Scope::Line,
        };
        assert_eq!(records(&changed), [Record::from(&one_way())]);
        let ended = Notification::Ended {
            line: "C01740".into(),
        };
        assert_eq!(records(&ended), []);
        // A train's cancellation comes from the stop's list, not from a record.
        let train = crate::trains::Train {
            line: "C01740".into(),
            mission: None,
            destination: "Gare Centrale".into(),
            from: "Les Aubépines".into(),
            aimed: "2026-11-03T06:52:00Z".parse().unwrap(),
            expected: "2026-11-03T06:52:00Z".parse().unwrap(),
        };
        assert_eq!(
            records(&Notification::TrainBack {
                train: train.clone()
            }),
            []
        );
        assert_eq!(records(&Notification::Late { train }), []);
    }

    #[test]
    fn a_window_that_closes_on_a_disrupted_line_says_which_and_how() {
        let labels = [LineLabel {
            code: "C01740".into(),
            name: "L".into(),
            mode: Mode::Train,
            color: None,
        }];
        assert_eq!(
            window_closed("C01740", Severity::Blocking, &labels),
            BTreeMap::from([
                ("type".to_string(), "window_closed".to_string()),
                ("line".to_string(), "L".to_string()),
                ("mode".to_string(), "train".to_string()),
                ("severity".to_string(), "blocking".to_string()),
            ])
        );
    }
}
