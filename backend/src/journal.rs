//! What the backend keeps for later, unlike the history, nothing dropped: each
//! train followed, as it ended, for the statistics, and notes on its own
//! judgments, to look back at them. One JSON object per line, appended to a
//! file of the data folder, a few kilobytes a day.

use std::fs::OpenOptions;
use std::io::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use jiff::Timestamp;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::calendar::Direction;
use crate::relevance::Bearing;

pub struct Journal<T> {
    path: PathBuf,
    kept: PhantomData<T>,
    /// Appending and writing it again, one at a time.
    writing: Mutex<()>,
}

/// A judgment of the backend's, noted to look back at it: when trains run late
/// without an alert, or the departures show the timetable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Note {
    /// A record of the way's lines that no leg was sure of, when first judged so.
    Doubt {
        at: Timestamp,
        direction: Direction,
        record: String,
        title: String,
        /// Its sections, "from -> to".
        sections: Vec<String>,
        bearing: Bearing,
    },
    /// A train ride shown by its timetable, its stops not read live.
    Timetable { at: Timestamp, line: String },
}

impl<T: Serialize + DeserializeOwned> Journal<T> {
    pub fn open(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            kept: PhantomData,
            writing: Mutex::new(()),
        }
    }

    /// Adds `entries` at the end. A failure is logged: it must not stop the alerts.
    pub fn append(&self, entries: &[T]) {
        if entries.is_empty() {
            return;
        }
        let mut text = String::new();
        for entry in entries {
            text.push_str(&serde_json::to_string(entry).expect("an entry serializes"));
            text.push('\n');
        }
        let _writing = self.writing.lock().expect("no thread panics while writing");
        let written = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| file.write_all(text.as_bytes()));
        if let Err(error) = written {
            eprintln!(
                "cannot keep {} entries in {}: {error}",
                entries.len(),
                self.path.display()
            );
        }
    }

    /// The last `count` entries, the latest first. A line that cannot be read is left out.
    /// Keeps the entries `keep` keeps, in order, and says how many went: the
    /// journal is written again aside, then put in place. A line that cannot be
    /// read stays as it is.
    pub fn retain(&self, keep: impl Fn(&T) -> bool) -> std::io::Result<usize> {
        let _writing = self.writing.lock().expect("no thread panics while writing");
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let mut kept = String::new();
        let mut gone = 0;
        for line in text.lines() {
            match serde_json::from_str::<T>(line) {
                Ok(entry) if !keep(&entry) => gone += 1,
                _ => {
                    kept.push_str(line);
                    kept.push('\n');
                }
            }
        }
        if gone > 0 {
            let aside = self.path.with_extension("tmp");
            std::fs::write(&aside, kept).and_then(|()| std::fs::rename(&aside, &self.path))?;
        }
        Ok(gone)
    }

    pub fn latest(&self, count: usize) -> Vec<T> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        text.lines()
            .rev()
            .filter_map(|line| serde_json::from_str(line).ok())
            .take(count)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trains::{Ended, Outcome, Train};
    use jiff::civil::date;

    fn ended(aimed: &str, outcome: Outcome) -> Ended {
        let aimed = format!("2026-11-03T{aimed}:00Z").parse().unwrap();
        Ended {
            day: date(2026, 11, 3),
            direction: Direction::Outbound,
            train: Train {
                line: "C01740".into(),
                mission: Some("PALO".into()),
                destination: "Gare Centrale".into(),
                from: "Les Aubépines".into(),
                aimed,
                expected: aimed,
            },
            outcome,
            came_back: false,
        }
    }

    #[test]
    fn trains_are_appended_and_read_back_the_latest_first() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::open(&dir.path().join("journal.jsonl"));
        assert_eq!(journal.latest(10), []);
        journal.append(&[
            ended("06:52", Outcome::Ran),
            ended("07:07", Outcome::Missing),
        ]);
        journal.append(&[]);
        journal.append(&[ended("07:22", Outcome::Announced)]);
        assert_eq!(
            journal.latest(2),
            [
                ended("07:22", Outcome::Announced),
                ended("07:07", Outcome::Missing)
            ]
        );
        // Read again after a restart, as written.
        let text = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert_eq!(
            text.lines().next().unwrap(),
            r#"{"day":"2026-11-03","direction":"outbound","line":"C01740","mission":"PALO","destination":"Gare Centrale","from":"Les Aubépines","aimed":"2026-11-03T06:52:00Z","expected":"2026-11-03T06:52:00Z","outcome":"ran","came_back":false}"#
        );
    }

    #[test]
    fn entries_taken_out_leave_the_rest_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let journal = Journal::open(&path);
        // Nothing kept yet: nothing to take out, nothing written.
        assert_eq!(journal.retain(|_| false).unwrap(), 0);
        assert!(!path.exists());
        journal.append(&[
            ended("06:52", Outcome::Ran),
            ended("07:07", Outcome::Missing),
            ended("07:22", Outcome::Ran),
        ]);
        // A line that cannot be read stays as it is.
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{not json\n");
        std::fs::write(&path, text).unwrap();
        let ran = |entry: &Ended| entry.outcome == Outcome::Ran;
        assert_eq!(journal.retain(ran).unwrap(), 1);
        assert_eq!(
            journal.latest(5),
            [ended("07:22", Outcome::Ran), ended("06:52", Outcome::Ran)]
        );
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("{not json")
        );
        // Nothing to take out: left as it is; and appended to after.
        assert_eq!(journal.retain(|_| true).unwrap(), 0);
        journal.append(&[ended("07:37", Outcome::Ran)]);
        assert_eq!(journal.latest(1), [ended("07:37", Outcome::Ran)]);
    }

    #[test]
    fn a_journal_that_cannot_be_written_again_says_so_and_stays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let journal = Journal::open(&path);
        journal.append(&[ended("06:52", Outcome::Ran)]);
        // Where the new journal would be written first, a folder.
        std::fs::create_dir(path.with_extension("tmp")).unwrap();
        assert!(journal.retain(|_| false).is_err());
        assert_eq!(journal.latest(5), [ended("06:52", Outcome::Ran)]);
    }

    #[test]
    fn a_line_that_cannot_be_read_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let journal = Journal::open(&path);
        journal.append(&[ended("06:52", Outcome::Ran)]);
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{not json\n");
        std::fs::write(&path, text).unwrap();
        journal.append(&[ended("07:07", Outcome::Ran)]);
        assert_eq!(
            journal.latest(5),
            [ended("07:07", Outcome::Ran), ended("06:52", Outcome::Ran)]
        );
    }

    #[test]
    fn a_folder_that_cannot_be_written_loses_the_trains_not_the_alerts() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be.
        let path = dir.path().join("journal.jsonl");
        std::fs::create_dir(&path).unwrap();
        let journal = Journal::open(&path);
        journal.append(&[ended("06:52", Outcome::Ran)]);
        assert_eq!(journal.latest(5), []);
    }
}
