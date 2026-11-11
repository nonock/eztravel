//! The statistics of the trains kept in the journal, office days only: per way
//! and line, how many ran on time, late, or not at all, in all, by departure
//! time, by weekday and day by day; and each day, how the way's usual train
//! ran. Trains leaving together, the halves of one train that part further
//! on, count as one, the best of them: if one left, the departure was there.
//! Besides, from what was sent, the days each line was told disrupted.

use std::collections::{BTreeMap, BTreeSet};

use jiff::civil::{Date, Time};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use serde::Serialize;

use crate::calendar::Direction;
use crate::history::Entry;
use crate::leaving::LATEST;
use crate::profile::UsualTrains;
use crate::push::{self, LineLabel};
use crate::trains::{Ended, Outcome};

/// Five minutes behind its time or more, a train ran late, as the operator counts.
pub const LATE: SignedDuration = SignedDuration::from_mins(5);

#[derive(Debug, PartialEq, Serialize)]
pub struct Stats {
    /// The first day kept, if any.
    pub since: Option<Date>,
    /// The way to work first, then home; each line in the order first kept.
    pub ways: Vec<Way>,
    pub disruptions: Disruptions,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Way {
    pub direction: Direction,
    pub line: String,
    pub mode: &'static str,
    pub color: Option<String>,
    /// The way's usual train, by its time, when set and on this line.
    pub usual: Option<Time>,
    #[serde(flatten)]
    pub count: Count,
    /// Each departure time, in Paris, kept one day in two at least: the
    /// trains of an evening home late are left out. The earliest first.
    pub times: Vec<Slot>,
    /// Each weekday with a train kept, Monday first.
    pub weekdays: Vec<Weekday>,
    /// The latest day first.
    pub days: Vec<Day>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Slot {
    pub time: Time,
    #[serde(flatten)]
    pub count: Count,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Weekday {
    /// From 1, Monday, to 7, Sunday.
    pub weekday: i8,
    /// How many of these days had a train kept.
    pub days: usize,
    #[serde(flatten)]
    pub count: Count,
}

/// The lines told disrupted, from the history of what was sent: as it keeps
/// the latest entries only, `since` moves on as the oldest go.
#[derive(Debug, PartialEq, Serialize)]
pub struct Disruptions {
    pub since: Option<Date>,
    /// In the order of the lines described, then the others by name.
    pub lines: Vec<Disrupted>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Disrupted {
    pub line: String,
    pub mode: String,
    pub color: Option<String>,
    /// The days, in Paris, with an alert.
    pub days: usize,
    /// The alerts sent: a disruption begun, or trains late with no record to tell why.
    pub alerts: usize,
}

/// What was sent that tells a line disrupted.
const ALERTS: [&str; 2] = ["started", "line_late"];

#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Count {
    pub trains: usize,
    pub on_time: usize,
    pub late: usize,
    pub cancelled: usize,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Day {
    pub day: Date,
    #[serde(flatten)]
    pub count: Count,
    /// The usual train of that day: the first from its time, up to half an hour after.
    pub usual: Option<UsualRun>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct UsualRun {
    pub aimed: Timestamp,
    pub run: Run,
    /// Whole minutes behind its time; none when cancelled.
    pub late: i64,
}

/// From the best to the worst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Run {
    OnTime,
    Late,
    Cancelled,
}

/// How `ended` ran.
fn run(ended: &Ended) -> Run {
    match ended.outcome {
        Outcome::Missing | Outcome::Announced => Run::Cancelled,
        Outcome::Ran if ended.train.expected.duration_since(ended.train.aimed) >= LATE => Run::Late,
        Outcome::Ran => Run::OnTime,
    }
}

impl Count {
    fn add(&mut self, ended: &Ended) {
        self.trains += 1;
        match run(ended) {
            Run::OnTime => self.on_time += 1,
            Run::Late => self.late += 1,
            Run::Cancelled => self.cancelled += 1,
        }
    }
}

/// The statistics of `journal` and `history`, in any order. `reminded` gives
/// the line of each way's ride the reminder is for, where the usual train is.
pub fn stats(
    journal: &[Ended],
    history: &[Entry],
    usual: &UsualTrains,
    reminded: impl Fn(Direction) -> Option<String>,
    labels: &[LineLabel],
    tz: &TimeZone,
) -> Stats {
    let mut kept: Vec<&Ended> = journal.iter().collect();
    kept.sort_by_key(|ended| (ended.day, ended.train.aimed));
    let mut ways: Vec<Way> = vec![];
    for direction in [Direction::Outbound, Direction::Inbound] {
        let of_way: Vec<&Ended> = kept
            .iter()
            .copied()
            .filter(|ended| ended.direction == direction)
            .collect();
        let mut lines: Vec<&str> = vec![];
        for ended in &of_way {
            if !lines.contains(&ended.train.line.as_str()) {
                lines.push(&ended.train.line);
            }
        }
        for line in lines {
            let mut trains: Vec<&Ended> = vec![];
            for ended in of_way
                .iter()
                .copied()
                .filter(|ended| ended.train.line == line)
            {
                match trains.last_mut() {
                    Some(last)
                        if (last.day, last.train.aimed) == (ended.day, ended.train.aimed) =>
                    {
                        if (run(ended), ended.train.expected) < (run(last), last.train.expected) {
                            *last = ended;
                        }
                    }
                    _ => trains.push(ended),
                }
            }
            let usual_time = usual
                .of(direction)
                .filter(|_| reminded(direction).as_deref() == Some(line));
            let mut count = Count::default();
            let mut days: Vec<Day> = vec![];
            // Each with the days it was kept, in order.
            let mut slots: BTreeMap<Time, (Count, Vec<Date>)> = BTreeMap::new();
            let mut weekdays: BTreeMap<i8, (Count, Vec<Date>)> = BTreeMap::new();
            for ended in &trains {
                count.add(ended);
                let time = ended.train.aimed.to_zoned(tz.clone()).time();
                let weekday = ended.day.weekday().to_monday_one_offset();
                for (count, seen) in [
                    slots.entry(time).or_default(),
                    weekdays.entry(weekday).or_default(),
                ] {
                    count.add(ended);
                    if seen.last() != Some(&ended.day) {
                        seen.push(ended.day);
                    }
                }
                if days.last().is_none_or(|day| day.day != ended.day) {
                    days.push(Day {
                        day: ended.day,
                        count: Count::default(),
                        usual: None,
                    });
                }
                days.last_mut()
                    .expect("a day was just added")
                    .count
                    .add(ended);
            }
            for day in &mut days {
                day.usual = usual_time.and_then(|time| {
                    let from = day
                        .day
                        .to_datetime(time)
                        .to_zoned(tz.clone())
                        .expect("a day's local time is within range")
                        .timestamp();
                    let first =
                        trains
                            .iter()
                            .filter(|ended| ended.day == day.day)
                            .find(|ended| {
                                ended.train.aimed >= from && ended.train.aimed <= from + LATEST
                            })?;
                    let run = run(first);
                    Some(UsualRun {
                        aimed: first.train.aimed,
                        run,
                        late: if run == Run::Cancelled {
                            0
                        } else {
                            first
                                .train
                                .expected
                                .duration_since(first.train.aimed)
                                .as_mins()
                        },
                    })
                });
            }
            let times = slots
                .into_iter()
                .filter(|(_, (_, seen))| seen.len() * 2 >= days.len())
                .map(|(time, (count, _))| Slot { time, count })
                .collect();
            let weekdays = weekdays
                .into_iter()
                .map(|(weekday, (count, seen))| Weekday {
                    weekday,
                    days: seen.len(),
                    count,
                })
                .collect();
            days.reverse();
            let (name, mode, color) = push::label(line, labels);
            ways.push(Way {
                direction,
                line: name,
                mode,
                color,
                usual: usual_time,
                count,
                times,
                weekdays,
                days,
            });
        }
    }
    Stats {
        since: kept.first().map(|ended| ended.day),
        ways,
        disruptions: disruptions(history, labels, tz),
    }
}

/// The days each line was told disrupted, and the alerts, in `history`.
fn disruptions(history: &[Entry], labels: &[LineLabel], tz: &TimeZone) -> Disruptions {
    let day = |at: Timestamp| at.to_zoned(tz.clone()).date();
    // By the line's place among those described, then its name: its mode as sent, its days and alerts.
    let mut lines: BTreeMap<(usize, String), (String, BTreeSet<Date>, usize)> = BTreeMap::new();
    for entry in history {
        let alert = entry
            .data
            .get("type")
            .is_some_and(|kind| ALERTS.contains(&kind.as_str()));
        let Some(name) = entry.data.get("line").filter(|_| alert) else {
            continue;
        };
        let place = labels
            .iter()
            .position(|label| &label.name == name)
            .unwrap_or(labels.len());
        let (_, days, alerts) = lines.entry((place, name.clone())).or_insert_with(|| {
            let mode = entry.data.get("mode").cloned().unwrap_or_default();
            (mode, BTreeSet::new(), 0)
        });
        days.insert(day(entry.at));
        *alerts += 1;
    }
    Disruptions {
        since: history.iter().map(|entry| entry.at).min().map(day),
        lines: lines
            .into_iter()
            .map(|((place, line), (mode, days, alerts))| {
                let (mode, color) = match labels.get(place) {
                    Some(label) => {
                        let (_, mode, color) = push::label(&label.code, labels);
                        (mode.to_string(), color)
                    }
                    None => (mode, None),
                };
                Disrupted {
                    line,
                    mode,
                    color,
                    days: days.len(),
                    alerts,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::Mode;
    use crate::trains::Train;
    use jiff::civil::{date, time};

    const DAY: Date = date(2026, 11, 3);

    /// A train of `line` on the way `direction`, leaving `aimed` UTC, `late` minutes behind.
    fn ended(
        day: Date,
        direction: Direction,
        line: &str,
        aimed: &str,
        late: i64,
        outcome: Outcome,
    ) -> Ended {
        let aimed: Timestamp = format!("{day}T{aimed}:00Z").parse().unwrap();
        Ended {
            day,
            direction,
            train: Train {
                line: line.into(),
                mission: Some("PALO".into()),
                destination: "Gare Centrale".into(),
                from: "Les Aubépines".into(),
                aimed,
                expected: aimed + SignedDuration::from_mins(late),
            },
            outcome,
            came_back: false,
        }
    }

    fn paris() -> TimeZone {
        TimeZone::get("Europe/Paris").unwrap()
    }

    fn count(trains: usize, on_time: usize, late: usize, cancelled: usize) -> Count {
        Count {
            trains,
            on_time,
            late,
            cancelled,
        }
    }

    #[test]
    fn trains_run_on_time_under_five_minutes_late_and_cancelled_whatever_why() {
        use Direction::Outbound;
        let next = date(2026, 11, 4);
        let journal = [
            ended(next, Outbound, "C01740", "06:52", 0, Outcome::Ran),
            ended(DAY, Outbound, "C01740", "06:52", 4, Outcome::Ran),
            ended(DAY, Outbound, "C01740", "07:07", 5, Outcome::Ran),
            ended(DAY, Outbound, "C01740", "07:22", 0, Outcome::Missing),
            ended(DAY, Outbound, "C01740", "07:37", 0, Outcome::Announced),
        ];
        let stats = stats(
            &journal,
            &[],
            &UsualTrains::default(),
            |_| None,
            &[],
            &paris(),
        );
        assert_eq!(stats.since, Some(DAY));
        assert_eq!(stats.ways.len(), 1);
        let way = &stats.ways[0];
        assert_eq!(
            (way.line.as_str(), way.mode, way.usual),
            ("C01740", "line", None)
        );
        assert_eq!(way.count, count(5, 2, 1, 2));
        let days: Vec<(Date, &Count)> = way.days.iter().map(|day| (day.day, &day.count)).collect();
        assert_eq!(
            days,
            [(next, &count(1, 1, 0, 0)), (DAY, &count(4, 1, 1, 2))]
        );
    }

    #[test]
    fn the_usual_train_is_the_first_from_its_time_up_to_half_an_hour_after() {
        use Direction::{Inbound, Outbound};
        let (second, third) = (date(2026, 11, 4), date(2026, 11, 5));
        let journal = [
            ended(DAY, Outbound, "C01740", "06:50", 0, Outcome::Ran),
            ended(DAY, Outbound, "C01740", "06:52", 6, Outcome::Ran),
            ended(second, Outbound, "C01740", "07:30", 0, Outcome::Ran),
            ended(third, Outbound, "C01740", "06:55", 0, Outcome::Missing),
            ended(DAY, Outbound, "C02000", "06:53", 0, Outcome::Ran),
            ended(DAY, Inbound, "C01740", "16:52", 0, Outcome::Ran),
        ];
        // At 7:52 in Paris, on the ride of line L.
        let usual = UsualTrains {
            outbound: Some(time(7, 52, 0, 0)),
            inbound: None,
        };
        let reminded = |_| Some("C01740".to_string());
        let stats = stats(&journal, &[], &usual, reminded, &[], &paris());
        let usual_of = |way: &Way| -> Vec<Option<(Timestamp, Run, i64)>> {
            way.days
                .iter()
                .map(|day| day.usual.as_ref().map(|run| (run.aimed, run.run, run.late)))
                .collect()
        };
        let train_l = &stats.ways[0];
        assert_eq!(train_l.usual, Some(time(7, 52, 0, 0)));
        let at =
            |day: Date, time: &str| -> Timestamp { format!("{day}T{time}:00Z").parse().unwrap() };
        assert_eq!(
            usual_of(train_l),
            [
                Some((at(third, "06:55"), Run::Cancelled, 0)),
                // More than half an hour after: none that day.
                None,
                Some((at(DAY, "06:52"), Run::Late, 6)),
            ]
        );
        // Another line of the way, and a way with no usual train: none.
        assert_eq!(
            (stats.ways[1].line.as_str(), stats.ways[1].usual),
            ("C02000", None)
        );
        assert_eq!(usual_of(&stats.ways[1]), [None]);
        assert_eq!(
            (stats.ways[2].direction, stats.ways[2].usual),
            (Inbound, None)
        );
    }

    #[test]
    fn each_time_kept_one_day_in_two_or_more_counts_by_its_time_in_paris() {
        use Direction::Outbound;
        // Paris was two hours ahead of UTC until 25 Oct., one after.
        let (before, after) = (date(2026, 10, 23), date(2026, 10, 27));
        let (third, fourth) = (date(2026, 10, 28), date(2026, 10, 29));
        let journal = [
            ended(before, Outbound, "C01740", "05:52", 0, Outcome::Ran),
            ended(after, Outbound, "C01740", "06:52", 6, Outcome::Ran),
            ended(third, Outbound, "C01740", "06:52", 0, Outcome::Missing),
            ended(fourth, Outbound, "C01740", "06:52", 0, Outcome::Ran),
            // On two days of four: kept.
            ended(after, Outbound, "C01740", "07:07", 0, Outcome::Ran),
            ended(third, Outbound, "C01740", "07:07", 0, Outcome::Ran),
            // On one day only, as on an evening home late: left out.
            ended(fourth, Outbound, "C01740", "07:22", 0, Outcome::Ran),
        ];
        let stats = stats(
            &journal,
            &[],
            &UsualTrains::default(),
            |_| None,
            &[],
            &paris(),
        );
        let times: Vec<(Time, &Count)> = stats.ways[0]
            .times
            .iter()
            .map(|slot| (slot.time, &slot.count))
            .collect();
        assert_eq!(
            times,
            [
                (time(7, 52, 0, 0), &count(4, 2, 1, 1)),
                (time(8, 7, 0, 0), &count(2, 2, 0, 0))
            ]
        );
    }

    #[test]
    fn trains_leaving_together_are_one_departure_the_best_of_them() {
        use Direction::Inbound;
        // Halves of one train that part further on, listed apart.
        let journal = [
            ended(DAY, Inbound, "C01740", "16:52", 0, Outcome::Announced),
            ended(DAY, Inbound, "C01740", "16:52", 1, Outcome::Ran),
            ended(DAY, Inbound, "C01740", "17:07", 2, Outcome::Ran),
            ended(DAY, Inbound, "C01740", "17:07", 6, Outcome::Ran),
            ended(DAY, Inbound, "C01740", "17:22", 0, Outcome::Missing),
        ];
        // At 17:52 in Paris.
        let usual = UsualTrains {
            outbound: None,
            inbound: Some(time(17, 52, 0, 0)),
        };
        let reminded = |_| Some("C01740".to_string());
        let stats = stats(&journal, &[], &usual, reminded, &[], &paris());
        let way = &stats.ways[0];
        assert_eq!(way.count, count(3, 2, 0, 1));
        assert_eq!(way.days[0].count, count(3, 2, 0, 1));
        let run = way.days[0].usual.as_ref().map(|run| (run.run, run.late));
        assert_eq!(run, Some((Run::OnTime, 1)));
        let times: Vec<&Count> = way.times.iter().map(|slot| &slot.count).collect();
        assert_eq!(
            times,
            [&count(1, 1, 0, 0), &count(1, 1, 0, 0), &count(1, 0, 0, 1)]
        );
    }

    #[test]
    fn each_weekday_counts_its_trains_and_its_days_monday_first() {
        use Direction::Inbound;
        // Tuesdays 3 and 10 Nov., Monday 9.
        let journal = [
            ended(
                date(2026, 11, 10),
                Inbound,
                "C01740",
                "17:52",
                5,
                Outcome::Ran,
            ),
            ended(DAY, Inbound, "C01740", "17:52", 0, Outcome::Ran),
            ended(DAY, Inbound, "C01740", "18:07", 0, Outcome::Ran),
            ended(
                date(2026, 11, 9),
                Inbound,
                "C01740",
                "17:52",
                0,
                Outcome::Announced,
            ),
        ];
        let stats = stats(
            &journal,
            &[],
            &UsualTrains::default(),
            |_| None,
            &[],
            &paris(),
        );
        let weekdays: Vec<(i8, usize, &Count)> = stats.ways[0]
            .weekdays
            .iter()
            .map(|weekday| (weekday.weekday, weekday.days, &weekday.count))
            .collect();
        assert_eq!(
            weekdays,
            [(1, 1, &count(1, 0, 0, 1)), (2, 2, &count(3, 2, 1, 0))]
        );
    }

    /// What was sent at `at`, of `kind`, on `line` and its mode, if any.
    fn sent(at: &str, kind: &str, line: Option<(&str, &str)>) -> Entry {
        let mut data = BTreeMap::from([("type".to_string(), kind.to_string())]);
        if let Some((line, mode)) = line {
            data.insert("line".into(), line.into());
            data.insert("mode".into(), mode.into());
        }
        Entry {
            at: at.parse().unwrap(),
            data,
            delivered: 1,
            records: vec![],
        }
    }

    #[test]
    fn a_line_was_disrupted_on_the_days_in_paris_it_was_told_so() {
        let labels = [
            LineLabel {
                code: "C01740".into(),
                name: "L".into(),
                mode: Mode::Train,
                color: Some("#7B61C9".into()),
            },
            LineLabel {
                code: "C02000".into(),
                name: "X".into(),
                mode: Mode::Metro,
                color: None,
            },
        ];
        let history = [
            // The oldest entry, whatever it is, says since when.
            sent(
                "2026-11-02T07:15:00Z",
                "window_closed",
                Some(("L", "train")),
            ),
            sent("2026-11-03T06:00:00Z", "started", Some(("L", "train"))),
            sent("2026-11-03T06:10:00Z", "changed", Some(("L", "train"))),
            sent("2026-11-03T06:20:00Z", "ended", Some(("L", "train"))),
            sent("2026-11-03T06:40:00Z", "started", Some(("X", "metro"))),
            // Again in the evening: one more alert, the same day.
            sent("2026-11-03T16:30:00Z", "started", Some(("L", "train"))),
            // Late with no record to tell why, past midnight in Paris: the next day.
            sent("2026-11-04T23:30:00Z", "line_late", Some(("L", "train"))),
            sent(
                "2026-11-06T07:00:00Z",
                "train_cancelled",
                Some(("L", "train")),
            ),
            sent("2026-11-06T07:05:00Z", "planned", None),
            // A line no longer described keeps the name and mode it was sent with.
            sent("2026-11-06T07:10:00Z", "started", Some(("Z", "metro"))),
        ];
        let stats = stats(
            &[],
            &history,
            &UsualTrains::default(),
            |_| None,
            &labels,
            &paris(),
        );
        let line = |line: &str, mode: &str, color: Option<&str>, days, alerts| Disrupted {
            line: line.into(),
            mode: mode.into(),
            color: color.map(Into::into),
            days,
            alerts,
        };
        assert_eq!(
            stats.disruptions,
            Disruptions {
                since: Some(date(2026, 11, 2)),
                lines: vec![
                    line("L", "train", Some("#7B61C9"), 2, 3),
                    line("X", "metro", None, 1, 1),
                    line("Z", "metro", None, 1, 1),
                ],
            }
        );
    }

    #[test]
    fn with_nothing_kept_there_is_nothing_to_say() {
        let stats = stats(&[], &[], &UsualTrains::default(), |_| None, &[], &paris());
        assert_eq!(
            stats,
            Stats {
                since: None,
                ways: vec![],
                disruptions: Disruptions {
                    since: None,
                    lines: vec![]
                },
            }
        );
    }
}
