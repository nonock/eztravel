//! Each train followed, as it ended, kept for the statistics: unlike the
//! history, nothing is dropped. One JSON object per line, appended to a file of
//! the data folder, a few kilobytes a day.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::trains::Ended;

pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn open(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    /// Adds `ended` at the end. A failure is logged: it must not stop the alerts.
    pub fn append(&self, ended: &[Ended]) {
        if ended.is_empty() {
            return;
        }
        let mut text = String::new();
        for train in ended {
            text.push_str(&serde_json::to_string(train).expect("a train serializes"));
            text.push('\n');
        }
        let written = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| file.write_all(text.as_bytes()));
        if let Err(error) = written {
            eprintln!(
                "cannot keep {} ended trains in {}: {error}",
                ended.len(),
                self.path.display()
            );
        }
    }

    /// The last `count` trains, the latest first. A line that cannot be read is left out.
    pub fn latest(&self, count: usize) -> Vec<Ended> {
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
    use crate::calendar::Direction;
    use crate::trains::{Outcome, Train};
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
