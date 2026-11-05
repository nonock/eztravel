//! Trains of a line running late with no record to tell why: at least
//! [`LATE_TRAINS`] of those leaving around now, [`LATE`] or more behind their
//! timetable, while no alert is under way on the line. Told once, then when no
//! train around now is late any more; a record on the line tells the rest.

use jiff::SignedDuration;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::alerts::Notification;
use crate::calendar::Direction;
use crate::trains::Train;

/// A train this far behind its timetable runs late.
pub const LATE: SignedDuration = SignedDuration::from_mins(5);
/// So many trains late tell the line runs late.
pub const LATE_TRAINS: usize = 3;
/// Around now: the trains leaving from this long before now to this long after.
pub const AROUND: SignedDuration = SignedDuration::from_mins(30);

/// The lines told late in the current window. Kept between polls, and across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lateness {
    window: Option<(Date, Direction)>,
    late: Vec<String>,
}

/// One poll's word on how the lines run: each with its trains around now, and
/// those with an alert under way.
pub fn judge(
    state: &mut Lateness,
    key: (Date, Direction),
    lines: &[(String, Vec<Train>)],
    alerted: &[String],
) -> Vec<Notification> {
    if state.window != Some(key) {
        state.window = Some(key);
        state.late.clear();
    }
    let mut notifications = vec![];
    for (line, trains) in lines {
        let late: Vec<Train> = trains
            .iter()
            .filter(|train| train.expected.duration_since(train.aimed) >= LATE)
            .cloned()
            .collect();
        let told = state.late.contains(line);
        if alerted.contains(line) {
            state.late.retain(|known| known != line);
        } else if !told && late.len() >= LATE_TRAINS {
            state.late.push(line.clone());
            notifications.push(Notification::LineLate {
                line: line.clone(),
                trains: late,
            });
        } else if told && late.is_empty() {
            state.late.retain(|known| known != line);
            notifications.push(Notification::LineOnTime { line: line.clone() });
        }
    }
    notifications
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::Timestamp;
    use jiff::civil::date;

    const KEY: (Date, Direction) = (date(2026, 11, 3), Direction::Outbound);
    const LINE: &str = "C01740";

    /// A train of the line leaving `index` ten minutes apart from 6:00 UTC, `late` minutes behind.
    fn train(index: usize, late: i64) -> Train {
        let aimed: Timestamp = "2026-11-03T06:00:00Z".parse().unwrap();
        let aimed = aimed + SignedDuration::from_mins(10 * index as i64);
        Train {
            line: LINE.into(),
            mission: Some("PALO".into()),
            destination: "Gare Centrale".into(),
            from: "Les Aubépines".into(),
            aimed,
            expected: aimed + SignedDuration::from_mins(late),
        }
    }

    /// The trains of the line around now, by how late each is.
    fn around(late: &[i64]) -> Vec<(String, Vec<Train>)> {
        let trains = late
            .iter()
            .enumerate()
            .map(|(index, late)| train(index, *late));
        vec![(LINE.to_string(), trains.collect())]
    }

    #[test]
    fn a_line_late_with_no_record_is_told_once_then_on_time_again() {
        let mut state = Lateness::default();
        // Two late: not yet.
        assert_eq!(judge(&mut state, KEY, &around(&[6, 7, 2]), &[]), []);
        assert_eq!(
            judge(&mut state, KEY, &around(&[6, 7, 12]), &[]),
            [Notification::LineLate {
                line: LINE.into(),
                trains: vec![train(0, 6), train(1, 7), train(2, 12)],
            }]
        );
        // Once.
        assert_eq!(judge(&mut state, KEY, &around(&[6, 7, 12, 9]), &[]), []);
        // One still late: not on time yet.
        assert_eq!(judge(&mut state, KEY, &around(&[0, 6]), &[]), []);
        assert_eq!(
            judge(&mut state, KEY, &around(&[0, 1]), &[]),
            [Notification::LineOnTime { line: LINE.into() }]
        );
        assert_eq!(judge(&mut state, KEY, &around(&[0, 1]), &[]), []);
    }

    #[test]
    fn a_record_on_the_line_tells_why_its_trains_are_late() {
        let alerted = [LINE.to_string()];
        let mut state = Lateness::default();
        assert_eq!(judge(&mut state, KEY, &around(&[6, 7, 12]), &alerted), []);
        // Told late, then a record: its end says the rest.
        let mut state = Lateness::default();
        judge(&mut state, KEY, &around(&[6, 7, 12]), &[]);
        assert_eq!(judge(&mut state, KEY, &around(&[6, 7, 12]), &alerted), []);
        assert_eq!(judge(&mut state, KEY, &around(&[0]), &[]), []);
    }

    #[test]
    fn each_window_starts_afresh() {
        let mut state = Lateness::default();
        judge(&mut state, KEY, &around(&[6, 7, 12]), &[]);
        let evening = (date(2026, 11, 3), Direction::Inbound);
        assert_eq!(judge(&mut state, evening, &around(&[0]), &[]), []);
        assert_eq!(
            judge(&mut state, evening, &around(&[6, 7, 12]), &[]).len(),
            1
        );
    }
}
