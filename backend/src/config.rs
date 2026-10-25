//! The backend's configuration: the commute, its calendar and watch windows.
//!
//! It describes one person's commute, so the real file stays out of the
//! repository; `config.example.toml` shows the format.

use jiff::tz::TimeZone;
use serde::Deserialize;

use crate::backups::Backup;
use crate::calendar::{Calendar, Direction, Schedule, WatchWindows};
use crate::push::LineLabel;
use crate::relevance::Leg;
use crate::school::Zone;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "paris")]
    pub time_zone: String,
    pub calendar: Calendar,
    pub windows: WatchWindows,
    /// Whose school holidays the app shows; none without.
    #[serde(default)]
    pub school_holidays: Option<Zone>,
    #[serde(default)]
    pub lines: Vec<LineLabel>,
    pub outbound: Vec<Leg>,
    pub inbound: Vec<Leg>,
    /// Shown while lines of the way they stand in for are disrupted.
    #[serde(default)]
    pub backups: Vec<Backup>,
}

fn paris() -> String {
    "Europe/Paris".into()
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|error| error.to_string())?;
        TimeZone::get(&config.time_zone)
            .map_err(|error| format!("time_zone \"{}\": {error}", config.time_zone))?;
        for direction in [Direction::Outbound, Direction::Inbound] {
            let window = config.windows.of(direction);
            if window.trains_from.is_some_and(|from| from > window.start) {
                let name = direction.name();
                return Err(format!("windows.{name}: trains_from comes after start"));
            }
        }
        for backup in &config.backups {
            let (name, legs) = (backup.direction.name(), config.legs(backup.direction));
            if backup.instead_of.is_empty() {
                return Err(format!("backups: an {name} backup needs instead_of"));
            }
            if backup.rides.is_empty() {
                return Err(format!("backups: an {name} backup needs rides"));
            }
            if let Some(line) = backup
                .instead_of
                .iter()
                .find(|line| !legs.iter().any(|leg| leg.line == **line))
            {
                return Err(format!("backups: {line} is not ridden {name}"));
            }
        }
        Ok(config)
    }

    pub fn time_zone(&self) -> TimeZone {
        TimeZone::get(&self.time_zone).expect("checked when the configuration was read")
    }

    pub fn schedule(&self) -> Schedule {
        Schedule {
            calendar: self.calendar.clone(),
            windows: self.windows.clone(),
            time_zone: self.time_zone(),
        }
    }

    /// The rides of the way `direction`, in order.
    pub fn legs(&self, direction: Direction) -> &[Leg] {
        match direction {
            Direction::Outbound => &self.outbound,
            Direction::Inbound => &self.inbound,
        }
    }

    /// Codes of the lines ridden, each once, in commute order.
    pub fn line_codes(&self) -> Vec<String> {
        let mut codes: Vec<String> = vec![];
        for leg in self.outbound.iter().chain(&self.inbound) {
            if !codes.contains(&leg.line) {
                codes.push(leg.line.clone());
            }
        }
        codes
    }

    /// The lines ridden, then those of the backups, each once: whose routes
    /// and disruptions are read.
    pub fn all_line_codes(&self) -> Vec<String> {
        let mut codes = self.line_codes();
        for ride in self.backups.iter().flat_map(|backup| &backup.rides) {
            if !codes.contains(&ride.line) {
                codes.push(ride.line.clone());
            }
        }
        codes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::Window;
    use crate::push::Mode;
    use jiff::civil::Weekday::{Friday, Monday, Thursday, Tuesday, Wednesday};
    use jiff::civil::{date, time};

    const EXAMPLE: &str = include_str!("../config.example.toml");

    #[test]
    fn the_example_file_reads_as_documented() {
        let config = Config::parse(EXAMPLE).unwrap();
        assert_eq!(
            config.calendar,
            Calendar {
                anchor: date(2026, 11, 4),
                first_week: vec![Tuesday, Thursday],
                second_week: vec![Monday, Wednesday, Friday],
                whit_monday_off: true,
                days: std::collections::BTreeMap::new(),
            }
        );
        assert_eq!(
            config.school_holidays,
            Some(crate::school::Zone {
                zone: "Zone C".into(),
                location: "Paris".into()
            })
        );
        assert_eq!(
            config.windows.outbound,
            Window {
                start: time(6, 45, 0, 0),
                end: time(8, 15, 0, 0),
                trains_from: Some(time(5, 30, 0, 0)),
            }
        );
        assert_eq!(
            config.windows.inbound,
            Window {
                start: time(16, 0, 0, 0),
                end: time(17, 30, 0, 0),
                trains_from: None,
            }
        );
        assert_eq!(
            config.lines[1],
            LineLabel {
                code: "C02000".into(),
                name: "X".into(),
                mode: Mode::Metro,
                color: Some("#E07A1F".into())
            }
        );
        assert_eq!(
            config.outbound[0],
            Leg {
                line: "C01740".into(),
                board: "stop_area:IDFM:71100".into(),
                alight: "stop_area:IDFM:71370".into(),
                reach: Some(12),
            }
        );
        assert_eq!(config.inbound.len(), 2);
        assert_eq!(config.outbound[1].reach, None);
        assert_eq!(config.time_zone(), TimeZone::get("Europe/Paris").unwrap());
        assert_eq!(config.line_codes(), ["C01740", "C02000"]);
        assert_eq!(
            config.backups,
            [Backup {
                direction: Direction::Outbound,
                instead_of: vec!["C02000".into()],
                rides: vec![Leg {
                    line: "C03000".into(),
                    board: "stop_area:IDFM:71390".into(),
                    alight: "stop_area:IDFM:71400".into(),
                    reach: None,
                }],
            }]
        );
        assert_eq!(config.all_line_codes(), ["C01740", "C02000", "C03000"]);
        let color = |code| crate::push::label(code, &config.lines).2;
        assert_eq!(color("C01740").as_deref(), Some("#7B61C9"));
        assert_eq!(color("C09999"), None);
    }

    #[test]
    fn a_line_color_is_optional_but_must_be_one() {
        let without = Config::parse(&EXAMPLE.replace("color = \"#7B61C9\"", "")).unwrap();
        assert_eq!(crate::push::label("C01740", &without.lines).2, None);
        for wrong in ["#7B61C", "7B61C9", "#7B61CZ", "purple"] {
            let error = Config::parse(&EXAMPLE.replace("#7B61C9", wrong)).unwrap_err();
            assert!(error.contains("color"), "{wrong}: {error}");
        }
        let number = Config::parse(&EXAMPLE.replace("\"#7B61C9\"", "5")).unwrap_err();
        assert!(number.contains("color"), "{number}");
    }

    #[test]
    fn optional_settings_have_defaults() {
        let short: Vec<&str> = EXAMPLE
            .lines()
            .filter(|line| !line.starts_with("time_zone") && !line.starts_with("whit_monday_off"))
            .collect();
        let config = Config::parse(&short.join("\n")).unwrap();
        assert_eq!(config.time_zone, "Europe/Paris");
        assert!(config.calendar.whit_monday_off);
        let start = EXAMPLE.find("[[backups]]").unwrap();
        let without = Config::parse(&EXAMPLE[..start]).unwrap();
        assert_eq!(without.backups, []);
        assert_eq!(without.all_line_codes(), without.line_codes());
    }

    #[test]
    fn a_backup_must_stand_in_for_lines_of_its_way() {
        let parse = |from: &str, to: &str| {
            assert!(EXAMPLE.contains(from), "{from}");
            Config::parse(&EXAMPLE.replace(from, to)).unwrap_err()
        };
        // Never shown: the morning way has no such line.
        let other = parse("instead_of = [\"C02000\"]", "instead_of = [\"C09999\"]");
        assert!(
            other.contains("C09999") && other.contains("outbound"),
            "{other}"
        );
        let none = parse("instead_of = [\"C02000\"]", "instead_of = []");
        assert!(none.contains("instead_of"), "{none}");
        let rides = &EXAMPLE[EXAMPLE.find("rides = [").unwrap()..];
        let empty = parse(&rides[..rides.find("\n]").unwrap() + 2], "rides = []");
        assert!(empty.contains("rides"), "{empty}");
        let field = parse("instead_of", "avoids");
        assert!(field.contains("avoids"), "{field}");
        let way = parse("direction = \"outbound\"", "direction = \"morning\"");
        assert!(way.contains("morning"), "{way}");
        // The evening way rides X too.
        let evening = EXAMPLE.replace("direction = \"outbound\"", "direction = \"inbound\"");
        let evening = Config::parse(&evening).unwrap();
        assert_eq!(evening.backups[0].direction, Direction::Inbound);
    }

    #[test]
    fn mistakes_are_reported_with_what_went_wrong() {
        let weekday = Config::parse(&EXAMPLE.replace("\"tuesday\"", "\"tuesdy\"")).unwrap_err();
        assert!(weekday.contains("tuesdy"), "{weekday}");
        let zone = Config::parse(&EXAMPLE.replace("Europe/Paris", "Europe/Atlantis")).unwrap_err();
        assert!(zone.contains("Europe/Atlantis"), "{zone}");
        // A misspelt setting is not silently ignored.
        let field =
            Config::parse(&EXAMPLE.replace("whit_monday_off", "whit_monday_of")).unwrap_err();
        assert!(field.contains("whit_monday_of"), "{field}");
        // School holidays are optional, and read as written.
        let section = "[school_holidays]\nzone = \"Zone C\"\nlocation = \"Paris\"\n";
        assert!(EXAMPLE.contains(section));
        assert_eq!(
            Config::parse(&EXAMPLE.replace(section, ""))
                .unwrap()
                .school_holidays,
            None
        );
        let town = Config::parse(&EXAMPLE.replace("location = \"Paris\"", "town = \"Paris\""))
            .unwrap_err();
        assert!(town.contains("town"), "{town}");
        // Days are set from the app only: two sources would contradict each other.
        let days = EXAMPLE.replace(
            "whit_monday_off = true",
            "whit_monday_off = true
days = { 2026-12-31 = \"office\" }",
        );
        assert!(Config::parse(&days).unwrap_err().contains("days"));
        let window = Config::parse(&EXAMPLE.replace("outbound = { start", "outbound = { begin"))
            .unwrap_err();
        assert!(window.contains("begin"), "{window}");
        let windows =
            Config::parse(&EXAMPLE.replace("inbound = { start", "evening = { start")).unwrap_err();
        assert!(windows.contains("evening"), "{windows}");
        // The usual train of each way is set in the app, as the days are.
        let leave = format!("{EXAMPLE}\n[leave]\noutbound = \"07:30\"\n");
        assert!(Config::parse(&leave).unwrap_err().contains("leave"));
        // Trains are followed before their window, not after it opens.
        let late =
            Config::parse(&EXAMPLE.replace("trains_from = \"05:30\"", "trains_from = \"07:00\""))
                .unwrap_err();
        assert!(late.contains("trains_from"), "{late}");
    }

    #[test]
    fn the_schedule_is_the_calendar_and_windows_in_their_time_zone() {
        let config = Config::parse(EXAMPLE).unwrap();
        let schedule = config.schedule();
        assert_eq!(
            (schedule.calendar, schedule.windows, schedule.time_zone),
            (
                config.calendar.clone(),
                config.windows.clone(),
                config.time_zone()
            )
        );
    }
}
