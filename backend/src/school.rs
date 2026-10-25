//! School holidays of the user's zone, from the French open data: some lines
//! run fewer trains then, and the app says so.
//!
//! Read from data.education.gouv.fr once a day, or again within the hour after a
//! failure, apart from the API: an answer never waits for that site. Until a
//! read succeeds, no day is known as a school holiday.

use std::sync::Mutex;

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ToSpan};
use serde::Deserialize;

use crate::http;

pub const SCHOOL_URL: &str = "https://data.education.gouv.fr";

const RECORDS: &str = "/api/explore/v2.1/catalog/datasets/fr-en-calendrier-scolaire/records";

/// Whose school holidays, as the open data names them: "Zone C" and "Paris".
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    pub zone: String,
    pub location: String,
}

/// A period of school holidays, by its local days, both included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holiday {
    pub name: String,
    pub first: Date,
    pub last: Date,
}

#[derive(Deserialize)]
struct Records {
    results: Vec<Record>,
}

#[derive(Deserialize)]
struct Record {
    description: String,
    #[serde(default)]
    population: Option<String>,
    start_date: Timestamp,
    end_date: Timestamp,
}

/// Each period runs from the midnight it starts, local time, to the midnight
/// classes resume; one that starts and ends at once is that day only. The
/// teachers' periods are left out.
pub fn parse(json: &str, time_zone: &TimeZone) -> Result<Vec<Holiday>, String> {
    let records: Records = serde_json::from_str(json).map_err(|error| error.to_string())?;
    Ok(records
        .results
        .into_iter()
        .filter(|record| record.population.as_deref() != Some("Enseignants"))
        .map(|record| {
            let first = record.start_date.to_zoned(time_zone.clone()).date();
            let resume = record.end_date.to_zoned(time_zone.clone()).date();
            Holiday {
                name: record.description,
                first,
                last: if resume > first {
                    resume - 1.day()
                } else {
                    first
                },
            }
        })
        .collect())
}

/// The name of the school holidays `date` is in, if any.
pub fn on(holidays: &[Holiday], date: Date) -> Option<&str> {
    holidays
        .iter()
        .find(|holiday| holiday.first <= date && date <= holiday.last)
        .map(|holiday| holiday.name.as_str())
}

/// How long until the next read, after one that went well or not.
pub fn next_read(read: bool) -> SignedDuration {
    if read {
        SignedDuration::from_hours(24)
    } else {
        SignedDuration::from_hours(1)
    }
}

/// The school holidays as last read, for the API to share.
pub struct SchoolHolidays {
    client: reqwest::Client,
    base: String,
    zone: Option<Zone>,
    time_zone: TimeZone,
    known: Mutex<Vec<Holiday>>,
}

impl SchoolHolidays {
    pub fn new(zone: Option<Zone>, base: &str, time_zone: TimeZone) -> Self {
        Self {
            client: http::client(http::TIMEOUT),
            base: base.trim_end_matches('/').to_string(),
            zone,
            time_zone,
            known: Mutex::default(),
        }
    }

    /// Reads the zone's holidays from this month on; whether that went well.
    /// Without a zone there is nothing to read. A failed read keeps what was known.
    pub async fn refresh(&self, now: Timestamp) -> bool {
        let Some(zone) = &self.zone else {
            return true;
        };
        let month = now.to_zoned(self.time_zone.clone()).date().first_of_month();
        let filter = format!(
            r#"zones = "{}" and location = "{}" and end_date >= "{month}""#,
            zone.zone, zone.location
        );
        let read = async {
            let url = reqwest::Url::parse_with_params(
                &format!("{}{RECORDS}", self.base),
                [
                    ("where", filter.as_str()),
                    ("order_by", "start_date"),
                    ("limit", "100"),
                ],
            )
            .map_err(|error| error.to_string())?;
            let text = async {
                self.client
                    .get(url)
                    .send()
                    .await?
                    .error_for_status()?
                    .text()
                    .await
            }
            .await
            .map_err(|error: reqwest::Error| error.to_string())?;
            parse(&text, &self.time_zone)
        };
        match read.await {
            Ok(holidays) => {
                self.set(holidays);
                true
            }
            Err(error) => {
                eprintln!("school holidays unavailable: {error}");
                false
            }
        }
    }

    pub fn set(&self, holidays: Vec<Holiday>) {
        *self
            .known
            .lock()
            .expect("no thread panics while holding the school holidays") = holidays;
    }

    pub fn known(&self) -> Vec<Holiday> {
        self.known
            .lock()
            .expect("no thread panics while holding the school holidays")
            .clone()
    }

    /// The school holidays `date` is in, with their first and last days, as last read.
    pub fn during(&self, date: Date) -> Option<Holiday> {
        self.known()
            .into_iter()
            .find(|holiday| holiday.first <= date && date <= holiday.last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn paris() -> TimeZone {
        TimeZone::get("Europe/Paris").unwrap()
    }

    fn zone() -> Zone {
        Zone {
            zone: "Zone C".into(),
            location: "Paris".into(),
        }
    }

    /// Records as data.education.gouv.fr gives them: each period from the
    /// midnight it starts, local time, to the midnight classes resume.
    const RECORDS: &str = r#"{"total_count": 4, "results": [
        {"description": "Vacances de la Toussaint", "population": "-", "start_date": "2026-10-16T22:00:00+00:00",
         "end_date": "2026-11-01T23:00:00+00:00", "location": "Paris", "zones": "Zone C", "annee_scolaire": "2026-2027"},
        {"description": "Pont de l'Ascension", "population": "-", "start_date": "2027-05-06T22:00:00+00:00",
         "end_date": "2027-05-06T22:00:00+00:00", "location": "Paris", "zones": "Zone C", "annee_scolaire": "2026-2027"},
        {"description": "Vacances d'Été", "population": "Enseignants", "start_date": "2027-07-02T22:00:00+00:00",
         "end_date": "2027-08-31T22:00:00+00:00", "location": "Paris", "zones": "Zone C", "annee_scolaire": "2026-2027"},
        {"description": "Vacances d'Été", "population": "Élèves", "start_date": "2027-07-02T22:00:00+00:00",
         "end_date": "2027-09-01T22:00:00+00:00", "location": "Paris", "zones": "Zone C", "annee_scolaire": "2026-2027"}
    ]}"#;

    fn holiday(name: &str, first: Date, last: Date) -> Holiday {
        Holiday {
            name: name.into(),
            first,
            last,
        }
    }

    #[test]
    fn each_period_runs_from_its_first_day_to_the_day_before_classes_resume() {
        assert_eq!(
            parse(RECORDS, &paris()).unwrap(),
            [
                // Over the change to winter time: from Saturday 17 October through Sunday 1 November.
                holiday(
                    "Vacances de la Toussaint",
                    date(2026, 10, 17),
                    date(2026, 11, 1)
                ),
                // Starting and ending at once: that day only.
                holiday("Pont de l'Ascension", date(2027, 5, 7), date(2027, 5, 7)),
                // The pupils' holidays, not the teachers'.
                holiday("Vacances d'Été", date(2027, 7, 3), date(2027, 9, 1)),
            ]
        );
        assert!(parse("not json", &paris()).is_err());
        assert!(parse(r#"{"results": [{"description": "x"}]}"#, &paris()).is_err());
    }

    #[test]
    fn a_day_is_in_the_holidays_from_the_first_through_the_last() {
        let holidays = [holiday(
            "Vacances de la Toussaint",
            date(2026, 10, 17),
            date(2026, 11, 1),
        )];
        assert_eq!(
            on(&holidays, date(2026, 10, 26)),
            Some("Vacances de la Toussaint")
        );
        for day in [date(2026, 10, 17), date(2026, 11, 1)] {
            assert!(on(&holidays, day).is_some(), "{day}");
        }
        for day in [date(2026, 10, 16), date(2026, 11, 2)] {
            assert_eq!(on(&holidays, day), None, "{day}");
        }
    }

    #[tokio::test]
    async fn the_zone_holidays_are_read_from_this_month_on() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/api/explore/v2.1/catalog/datasets/fr-en-calendrier-scolaire/records",
            ))
            .and(query_param(
                "where",
                r#"zones = "Zone C" and location = "Paris" and end_date >= "2026-10-01""#,
            ))
            .and(query_param("order_by", "start_date"))
            .and(query_param("limit", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RECORDS))
            .expect(1)
            .mount(&server)
            .await;
        let school = SchoolHolidays::new(Some(zone()), &server.uri(), paris());
        assert_eq!(school.known(), []);
        // Sunday 25 October 2026, 16:00 in Paris.
        assert!(
            school
                .refresh("2026-10-25T15:00:00Z".parse().unwrap())
                .await
        );
        assert_eq!(school.known().len(), 3);
        assert_eq!(
            school
                .during(date(2026, 10, 26))
                .map(|holiday| holiday.name),
            Some("Vacances de la Toussaint".into())
        );
    }

    #[tokio::test]
    async fn a_failed_read_keeps_what_was_known() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RECORDS))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let school = SchoolHolidays::new(Some(zone()), &server.uri(), paris());
        let now = "2026-10-25T15:00:00Z".parse().unwrap();
        assert!(school.refresh(now).await);
        assert!(!school.refresh(now).await);
        assert_eq!(school.known().len(), 3);
        // Nor does an answer that cannot be read.
        let garbled = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&garbled)
            .await;
        let school = SchoolHolidays::new(Some(zone()), &garbled.uri(), paris());
        assert!(!school.refresh(now).await);
        assert_eq!(school.known(), []);
        // Nor a site out of reach, nor an address that is not one.
        for base in ["http://127.0.0.1:9", "not an address"] {
            let school = SchoolHolidays::new(Some(zone()), base, paris());
            assert!(!school.refresh(now).await, "{base}");
        }
    }

    #[tokio::test]
    async fn without_a_zone_nothing_is_read() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RECORDS))
            .expect(0)
            .mount(&server)
            .await;
        let school = SchoolHolidays::new(None, &server.uri(), paris());
        assert!(
            school
                .refresh("2026-10-25T15:00:00Z".parse().unwrap())
                .await
        );
        assert_eq!(school.during(date(2026, 10, 26)), None);
    }

    #[test]
    fn read_again_the_next_day_or_within_the_hour_after_a_failure() {
        assert_eq!(next_read(true), SignedDuration::from_hours(24));
        assert_eq!(next_read(false), SignedDuration::from_hours(1));
    }

    #[test]
    fn holidays_can_be_set_as_read_elsewhere() {
        let school = SchoolHolidays::new(None, "http://127.0.0.1:9", paris());
        let toussaint = holiday(
            "Vacances de la Toussaint",
            date(2026, 10, 17),
            date(2026, 11, 1),
        );
        school.set(vec![toussaint.clone()]);
        assert_eq!(school.known(), std::slice::from_ref(&toussaint));
        // The holidays a day is in, whole: from the first day through the last.
        assert_eq!(school.during(date(2026, 10, 25)), Some(toussaint));
        assert_eq!(school.during(date(2026, 11, 2)), None);
    }
}
