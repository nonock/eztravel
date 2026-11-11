//! The statistics of the trains kept in the journal, office days only: per way
//! and line, how many ran on time, late, or not at all, in all and day by day;
//! and each day, how the way's usual train ran.

use jiff::civil::{Date, Time};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use serde::Serialize;

use crate::calendar::Direction;
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
    /// The latest day first.
    pub days: Vec<Day>,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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

/// The statistics of `journal`, in any order. `reminded` gives the line of
/// each way's ride the reminder is for, where the usual train is.
pub fn stats(
    journal: &[Ended],
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
            let trains: Vec<&Ended> = of_way
                .iter()
                .copied()
                .filter(|ended| ended.train.line == line)
                .collect();
            let usual_time = usual
                .of(direction)
                .filter(|_| reminded(direction).as_deref() == Some(line));
            let mut count = Count::default();
            let mut days: Vec<Day> = vec![];
            for ended in &trains {
                count.add(ended);
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
            days.reverse();
            let (name, mode, color) = push::label(line, labels);
            ways.push(Way {
                direction,
                line: name,
                mode,
                color,
                usual: usual_time,
                count,
                days,
            });
        }
    }
    Stats {
        since: kept.first().map(|ended| ended.day),
        ways,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let stats = stats(&journal, &UsualTrains::default(), |_| None, &[], &paris());
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
        let stats = stats(&journal, &usual, reminded, &[], &paris());
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
    fn with_nothing_kept_there_is_nothing_to_say() {
        let stats = stats(&[], &UsualTrains::default(), |_| None, &[], &paris());
        assert_eq!(
            stats,
            Stats {
                since: None,
                ways: vec![]
            }
        );
    }
}
