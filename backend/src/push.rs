//! Sends notifications to the phone through Firebase Cloud Messaging (HTTP v1).
//!
//! Messages carry data only: the app writes the title in the phone's language
//! from the type and parameters, and shows the operator's text as published.
//! The format of the data is fixed by the files in `fixtures/push/`.

use std::collections::BTreeMap;
use std::time::Duration;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jiff::{SignedDuration, Timestamp};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

use crate::alerts::Notification;
use crate::disruptions::{Disruption, Severity};
use crate::http;
use crate::trains::{Train, Why};

/// Firebase caps the data of a message at 4 KB: titles and texts are cut well below.
const TITLE_MAX_BYTES: usize = 300;
const TEXT_MAX_BYTES: usize = 3000;
/// An alert older than this is no longer worth showing.
const TIME_TO_LIVE: &str = "1800s";
const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Train,
    Metro,
}

/// How a line is named on screen, by its line code.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineLabel {
    pub code: String,
    pub name: String,
    pub mode: Mode,
    /// Its color on the home screen, as "#RRGGBB".
    #[serde(default, deserialize_with = "hex_color")]
    pub color: Option<String>,
}

fn hex_color<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let color = String::deserialize(deserializer)?;
    let digits = color
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()));
    match digits {
        Some(_) => Ok(Some(color)),
        None => Err(D::Error::custom(format!(
            "color \"{color}\" is not written \"#RRGGBB\""
        ))),
    }
}

/// The key file of a Google service account, as downloaded from Firebase.
#[derive(Clone, Deserialize)]
pub struct ServiceAccount {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    pub token_uri: String,
}

#[derive(Debug)]
pub enum PushError {
    /// The service account's private key cannot be used.
    Key,
    Http(reqwest::Error),
    /// Firebase no longer knows the device: the app was uninstalled or its token renewed.
    Unregistered,
    Status(u16, String),
}

/// The data of the message for `notification`. A line's message carries `until`, the end of
/// the watch window: the phone removes it then. A train's message goes away at the train's time.
pub fn data(
    notification: &Notification,
    labels: &[LineLabel],
    until: Timestamp,
) -> BTreeMap<String, String> {
    let until = until.to_string();
    let mut data = BTreeMap::new();
    let mut put = |key: &str, value: &str| {
        data.insert(key.to_string(), value.to_string());
    };
    let line = |code: &str, put: &mut dyn FnMut(&str, &str)| {
        let (name, mode) = label(code, labels);
        put("line", &name);
        put("mode", mode);
    };
    match notification {
        Notification::Started {
            line: code,
            severity,
            disruptions,
        } => {
            put("type", "started");
            line(code, &mut put);
            put("until", &until);
            describe(*severity, disruptions, &mut put);
        }
        Notification::Changed {
            line: code,
            from,
            to,
            disruptions,
        } => {
            put("type", "changed");
            line(code, &mut put);
            put("until", &until);
            put("previous", severity_name(*from));
            describe(*to, disruptions, &mut put);
        }
        Notification::Ended { line: code } => {
            put("type", "ended");
            line(code, &mut put);
            put("until", &until);
        }
        Notification::Planned { disruptions } => {
            put("type", "planned");
            put("count", &disruptions.len().to_string());
            let titles: Vec<&str> = disruptions
                .iter()
                .map(|disruption| disruption.title.as_str())
                .collect();
            put("titles", &truncate(&titles.join("\n"), TEXT_MAX_BYTES));
        }
        Notification::Outage => put("type", "outage"),
        Notification::Recovered => put("type", "recovered"),
        Notification::TrainCancelled { train, why } => {
            put("type", "train_cancelled");
            put(
                "reason",
                match why {
                    Why::Missing => "missing",
                    Why::Announced => "announced",
                },
            );
            describe_train(train, labels, &mut put);
        }
        Notification::TrainBack { train } => {
            put("type", "train_back");
            describe_train(train, labels, &mut put);
        }
    }
    data
}

fn describe_train(train: &Train, labels: &[LineLabel], put: &mut dyn FnMut(&str, &str)) {
    let (name, mode) = label(&train.line, labels);
    put("line", &name);
    put("mode", mode);
    if let Some(mission) = &train.mission {
        put("mission", mission);
    }
    put("destination", &train.destination);
    put("from", &train.from);
    let aimed = train.aimed.to_string();
    put("aimed", &aimed);
    put("until", &aimed);
}

/// How the line `code` is named on screen, and its mode: `train`, `metro`, or `line` when unlabelled.
pub fn label(code: &str, labels: &[LineLabel]) -> (String, &'static str) {
    match labels.iter().find(|label| label.code == code) {
        Some(label) => (
            label.name.clone(),
            if label.mode == Mode::Train {
                "train"
            } else {
                "metro"
            },
        ),
        None => (code.to_string(), "line"),
    }
}

/// Severity, and the words of the record that has it.
fn describe(severity: Severity, disruptions: &[Disruption], put: &mut dyn FnMut(&str, &str)) {
    put("severity", severity_name(severity));
    let shown = disruptions
        .iter()
        .find(|disruption| disruption.severity == severity)
        .or(disruptions.first());
    if let Some(record) = shown {
        put("title", &truncate(&record.title, TITLE_MAX_BYTES));
        put(
            "text",
            &truncate(&plain_text(&record.message), TEXT_MAX_BYTES),
        );
    }
}

pub fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Blocking => "blocking",
        _ => "disrupted",
    }
}

/// The body of an FCM v1 `messages:send` request.
pub fn message(device: &str, data: &BTreeMap<String, String>) -> serde_json::Value {
    // While the phone is offline, a newer message about the same line replaces the older one.
    let collapse_key = match (data.get("line"), data.get("type").map(String::as_str)) {
        // A train's own: it must not replace its line's state, nor another train.
        (Some(line), Some("train_cancelled" | "train_back")) => {
            format!(
                "train-{line}-{}",
                data.get("aimed").map_or("", String::as_str)
            )
        }
        (Some(line), _) => format!("line-{line}"),
        (None, Some("planned")) => "planned".to_string(),
        // An outage and its end.
        _ => "watch".to_string(),
    };
    serde_json::json!({
        "message": {
            "token": device,
            "data": data,
            "android": { "priority": "HIGH", "ttl": TIME_TO_LIVE, "collapse_key": collapse_key },
        }
    })
}

/// Operator messages are HTML: keep the text, one line per paragraph.
pub fn plain_text(html: &str) -> String {
    let mut text = String::new();
    let mut tag: Option<String> = None;
    for character in html.chars() {
        match (&mut tag, character) {
            (None, '<') => tag = Some(String::new()),
            (Some(name), '>') => {
                let name = name.trim().to_lowercase();
                if ["br", "/p", "/li", "/div"]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                {
                    text.push('\n');
                }
                tag = None;
            }
            (Some(name), _) => name.push(character),
            // In HTML a line break in the source is just a space.
            (None, '\n' | '\r' | '\t') => text.push(' '),
            (None, _) => text.push(character),
        }
    }
    decode_entities(&text)
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_entities(text: &str) -> String {
    let mut decoded = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start..];
        let entity = rest.find(';').map(|end| (&rest[1..end], end));
        let character = entity.and_then(|(name, _)| match name {
            "nbsp" => Some(' '),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => {
                let code = match name.strip_prefix("#x") {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => name
                        .strip_prefix('#')
                        .and_then(|decimal| decimal.parse().ok()),
                };
                code.and_then(char::from_u32)
            }
        });
        match (character, entity) {
            (Some(character), Some((_, end))) => {
                decoded.push(character);
                rest = &rest[end + 1..];
            }
            _ => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

/// Cuts `text` to at most `max_bytes` bytes without splitting a character, marking the cut with "…".
pub fn truncate(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// The signed JWT exchanged for an access token (OAuth 2.0 service account flow).
pub fn assertion(account: &ServiceAccount, now: Timestamp) -> Result<String, PushError> {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let issued = now.as_second();
    let claims = serde_json::json!({
        "iss": account.client_email,
        "scope": SCOPE,
        "aud": account.token_uri,
        "iat": issued,
        "exp": issued + 3600,
    });
    let signed = format!("{header}.{}", URL_SAFE_NO_PAD.encode(claims.to_string()));
    let pem_body: String = account
        .private_key
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .map(str::trim)
        .collect();
    let der = STANDARD.decode(pem_body).map_err(|_| PushError::Key)?;
    let key = RsaKeyPair::from_pkcs8(&der).map_err(|_| PushError::Key)?;
    let mut signature = vec![0; key.public_modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .expect("a valid key signs into a buffer of its own size");
    Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

pub struct Sender {
    http: reqwest::Client,
    account: ServiceAccount,
    fcm_url: String,
    token: Option<(String, Timestamp)>,
}

impl Sender {
    /// `fcm_url` is `https://fcm.googleapis.com`, or a test server.
    pub fn new(account: ServiceAccount, fcm_url: &str) -> Self {
        Self::with_timeout(account, fcm_url, http::TIMEOUT)
    }

    pub(crate) fn with_timeout(account: ServiceAccount, fcm_url: &str, timeout: Duration) -> Self {
        Self {
            http: http::client(timeout),
            account,
            fcm_url: fcm_url.trim_end_matches('/').to_string(),
            token: None,
        }
    }

    pub async fn send(
        &mut self,
        now: Timestamp,
        device: &str,
        data: &BTreeMap<String, String>,
    ) -> Result<(), PushError> {
        let token = self.access_token(now).await?;
        let url = format!(
            "{}/v1/projects/{}/messages:send",
            self.fcm_url, self.account.project_id
        );
        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&message(device, data))
            .send()
            .await
            .map_err(PushError::Http)?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        if status.as_u16() == 404 && body.contains("UNREGISTERED") {
            return Err(PushError::Unregistered);
        }
        Err(PushError::Status(status.as_u16(), body))
    }

    async fn access_token(&mut self, now: Timestamp) -> Result<String, PushError> {
        if let Some((token, expires)) = &self.token
            && now < *expires
        {
            return Ok(token.clone());
        }
        let assertion = assertion(&self.account, now)?;
        let response = self
            .http
            .post(&self.account.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await
            .map_err(PushError::Http)?;
        let status = response.status();
        if !status.is_success() {
            return Err(PushError::Status(
                status.as_u16(),
                response.text().await.unwrap_or_default(),
            ));
        }
        let granted: Granted = response.json().await.map_err(PushError::Http)?;
        // Renewed a minute early, so that a request never leaves with an expiring token.
        let expires = now + SignedDuration::from_secs(granted.expires_in - 60);
        self.token = Some((granted.access_token.clone(), expires));
        Ok(granted.access_token)
    }
}

#[derive(Deserialize)]
struct Granted {
    access_token: String,
    expires_in: i64,
}

/// Helpers shared by the tests of the modules that send notifications.
#[cfg(test)]
pub(crate) mod testing {
    use super::ServiceAccount;
    use aws_lc_rs::encoding::{AsDer, Pkcs8V1Der};
    use aws_lc_rs::rsa::{KeyPair, KeySize};
    use aws_lc_rs::signature::KeyPair as _;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use std::sync::OnceLock;

    /// A key made for the tests: the real one never leaves `local/`.
    pub(crate) fn test_key() -> &'static (String, Vec<u8>) {
        static KEY: OnceLock<(String, Vec<u8>)> = OnceLock::new();
        KEY.get_or_init(|| {
            let key = KeyPair::generate(KeySize::Rsa2048).unwrap();
            let der = AsDer::<Pkcs8V1Der>::as_der(&key).unwrap();
            let pem = format!(
                "-----BEGIN PRIVATE KEY-----
{}
-----END PRIVATE KEY-----
",
                STANDARD.encode(der.as_ref())
            );
            (pem, key.public_key().as_ref().to_vec())
        })
    }

    pub(crate) fn account(token_uri: &str) -> ServiceAccount {
        ServiceAccount {
            project_id: "demo-project".into(),
            client_email: "sender@demo-project.iam.gserviceaccount.com".into(),
            private_key: test_key().0.clone(),
            token_uri: token_uri.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{account, test_key};
    use super::*;
    use crate::disruptions::{Cause, Period};
    use crate::trains::{Train, Why};
    use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};
    use base64::Engine;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
    use jiff::civil::date;
    use wiremock::matchers::{body_json, body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn labels() -> Vec<LineLabel> {
        vec![
            LineLabel {
                code: "C01740".into(),
                name: "L".into(),
                mode: Mode::Train,
                color: None,
            },
            LineLabel {
                code: "C02000".into(),
                name: "X".into(),
                mode: Mode::Metro,
                color: None,
            },
        ]
    }

    /// The end of the example's morning window on Tuesday 3 November 2026: 8:15 in Paris.
    fn until() -> Timestamp {
        "2026-11-03T07:15:00Z".parse().unwrap()
    }

    fn fixture(name: &str) -> BTreeMap<String, String> {
        let path = format!(
            "{}/../fixtures/push/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn record(title: &str, severity: Severity, html: &str) -> Disruption {
        let at = date(2026, 11, 3).at(7, 0, 0, 0);
        Disruption {
            id: title.into(),
            lines: vec![],
            stops: vec![],
            periods: vec![Period { begin: at, end: at }],
            updated: at,
            cause: Cause::Incident,
            severity,
            title: title.into(),
            short_message: None,
            message: html.into(),
            sections: vec![],
        }
    }

    #[test]
    fn a_start_shows_the_worst_record() {
        let notification = Notification::Started {
            line: "C01740".into(),
            severity: Severity::Blocking,
            disruptions: vec![
                record(
                    "Ligne L : perturbations",
                    Severity::Disrupted,
                    "<p>Autre chose</p>",
                ),
                record(
                    "Ligne L : Nanterre <-> Cergy trafic interrompu",
                    Severity::Blocking,
                    "<p>Le trafic est interrompu entre Nanterre et Cergy jusqu&#39;à 20h.</p><p>Motif : panne d'un train.</p>",
                ),
            ],
        };
        assert_eq!(data(&notification, &labels(), until()), fixture("started"));
    }

    #[test]
    fn a_change_gives_both_severities() {
        let notification = Notification::Changed {
            line: "C02000".into(),
            from: Severity::Blocking,
            to: Severity::Disrupted,
            disruptions: vec![record(
                "Métro X : Reprise progressive / trafic reste perturbé",
                Severity::Unknown,
                "Le trafic reprend mais reste perturbé sur toute la ligne.",
            )],
        };
        assert_eq!(data(&notification, &labels(), until()), fixture("changed"));
    }

    #[test]
    fn an_end_names_the_line_only() {
        assert_eq!(
            data(
                &Notification::Ended {
                    line: "C01740".into()
                },
                &labels(),
                until()
            ),
            fixture("ended")
        );
    }

    #[test]
    fn planned_disruptions_are_listed_by_title() {
        let notification = Notification::Planned {
            disruptions: vec![
                record("Ligne L : offre adaptée", Severity::Disrupted, ""),
                record("Métro X : station fermée", Severity::Blocking, ""),
            ],
        };
        assert_eq!(data(&notification, &labels(), until()), fixture("planned"));
    }

    #[test]
    fn a_notification_without_record_has_no_text() {
        let started = Notification::Started {
            line: "C01740".into(),
            severity: Severity::Disrupted,
            disruptions: vec![],
        };
        let sent = data(&started, &labels(), until());
        assert_eq!(sent["severity"], "disrupted");
        assert!(!sent.contains_key("title") && !sent.contains_key("text"));
    }

    #[test]
    fn a_line_without_label_goes_by_its_code() {
        let ended = data(
            &Notification::Ended {
                line: "C09999".into(),
            },
            &labels(),
            until(),
        );
        assert_eq!(
            (ended["line"].as_str(), ended["mode"].as_str()),
            ("C09999", "line")
        );
    }

    #[test]
    fn long_texts_are_cut() {
        let long = "é".repeat(4000);
        let notification = Notification::Started {
            line: "C01740".into(),
            severity: Severity::Disrupted,
            disruptions: vec![record(&long, Severity::Disrupted, &long)],
        };
        let sent = data(&notification, &labels(), until());
        assert!(sent["title"].len() <= TITLE_MAX_BYTES);
        assert!(sent["text"].len() <= TEXT_MAX_BYTES);
        assert!(sent["text"].ends_with('…'));
    }

    #[test]
    fn plain_text_keeps_paragraphs_and_decodes_entities() {
        assert_eq!(
            plain_text(
                "<p>Trafic  perturbé&nbsp;:\n retards.</p>\n<p>Motif&nbsp;: <b>colis</b> &amp; &quot;bagage&quot; &lt;ici&gt;</p>"
            ),
            "Trafic perturbé : retards.\nMotif : colis & \"bagage\" <ici>"
        );
        assert_eq!(
            plain_text("Un<br>deux<br/>trois<br />quatre"),
            "Un\ndeux\ntrois\nquatre"
        );
        assert_eq!(
            plain_text("  sans balise &apos;ici&apos;  "),
            "sans balise 'ici'"
        );
        assert_eq!(plain_text("<p></p>"), "");
    }

    #[test]
    fn numeric_entities_are_decoded_and_unknown_ones_kept() {
        // The most frequent in the feed: é, à, ’ and –.
        assert_eq!(
            plain_text("&#233;t&#233; &#224; l&#8217;arr&#234;t &#x2013; fin"),
            "été à l’arrêt – fin"
        );
        assert_eq!(
            plain_text("A &foo; B & C &#xZZ; &#99999999;"),
            "A &foo; B & C &#xZZ; &#99999999;"
        );
    }

    #[test]
    fn truncate_never_splits_a_character() {
        assert_eq!(truncate("court", 10), "court");
        assert_eq!(truncate("abcdefgh", 6), "abc…");
        // "é" takes two bytes: cutting at 6 would split the third one.
        assert_eq!(truncate("éééééé", 6), "é…");
        assert_eq!(truncate("éééééé", 7), "éé…");
    }

    #[test]
    fn the_message_is_urgent_short_lived_and_grouped_by_line() {
        let body = message("device-token", &fixture("started"));
        assert_eq!(body["message"]["token"], "device-token");
        assert_eq!(body["message"]["data"]["severity"], "blocking");
        assert_eq!(body["message"]["android"]["priority"], "HIGH");
        assert_eq!(body["message"]["android"]["ttl"], "1800s");
        assert_eq!(body["message"]["android"]["collapse_key"], "line-L");
        assert_eq!(
            message("t", &fixture("planned"))["message"]["android"]["collapse_key"],
            "planned"
        );
        // A restored watch replaces the outage, not the planned disruptions.
        for name in ["outage", "recovered"] {
            assert_eq!(
                message("t", &fixture(name))["message"]["android"]["collapse_key"],
                "watch"
            );
        }
    }

    fn train(mission: Option<&str>) -> Train {
        Train {
            line: "C01740".into(),
            mission: mission.map(String::from),
            destination: "Gare Centrale".into(),
            from: "Les Aubépines".into(),
            aimed: "2026-11-03T06:52:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn a_train_is_named_by_its_line_mission_and_time_and_goes_away_then() {
        let cancelled = |why| Notification::TrainCancelled {
            train: train(Some("PALO")),
            why,
        };
        assert_eq!(
            data(&cancelled(Why::Missing), &labels(), until()),
            fixture("train_cancelled")
        );
        assert_eq!(
            data(&cancelled(Why::Announced), &labels(), until())["reason"],
            "announced"
        );
        let back = |mission| Notification::TrainBack {
            train: train(mission),
        };
        assert_eq!(
            data(&back(Some("PALO")), &labels(), until()),
            fixture("train_back")
        );
        assert!(!data(&back(None), &labels(), until()).contains_key("mission"));
    }

    #[test]
    fn a_train_back_replaces_its_cancellation_only() {
        for name in ["train_cancelled", "train_back"] {
            assert_eq!(
                message("t", &fixture(name))["message"]["android"]["collapse_key"],
                "train-L-2026-11-03T06:52:00Z"
            );
        }
    }

    #[test]
    fn an_outage_and_its_end_carry_their_type_only() {
        assert_eq!(
            data(&Notification::Outage, &labels(), until()),
            fixture("outage")
        );
        assert_eq!(
            data(&Notification::Recovered, &labels(), until()),
            fixture("recovered")
        );
    }

    const NOW: &str = "2026-11-03T06:30:00Z";

    #[test]
    fn the_assertion_is_a_signed_jwt_for_firebase_messaging() {
        let jwt = assertion(
            &account("https://oauth2.example/token"),
            NOW.parse().unwrap(),
        )
        .unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let decode = |part: &str| -> serde_json::Value {
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
        };
        assert_eq!(
            decode(parts[0]),
            serde_json::json!({ "alg": "RS256", "typ": "JWT" })
        );
        let now = NOW.parse::<Timestamp>().unwrap().as_second();
        assert_eq!(
            decode(parts[1]),
            serde_json::json!({
                "iss": "sender@demo-project.iam.gserviceaccount.com",
                "scope": SCOPE,
                "aud": "https://oauth2.example/token",
                "iat": now,
                "exp": now + 3600,
            })
        );
        let signed = format!("{}.{}", parts[0], parts[1]);
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &test_key().1)
            .verify(signed.as_bytes(), &signature)
            .unwrap();
    }

    /// The outcome of a call, in words, so that each case is one comparison.
    fn outcome<T>(result: Result<T, PushError>) -> String {
        match result {
            Ok(_) => "ok".into(),
            Err(PushError::Http(_)) => "http".into(),
            Err(error) => format!("{error:?}"),
        }
    }

    #[test]
    fn a_bad_key_is_reported() {
        let mut broken = account("https://oauth2.example/token");
        broken.private_key =
            "-----BEGIN PRIVATE KEY-----\nnot base64 !\n-----END PRIVATE KEY-----".into();
        assert_eq!(outcome(assertion(&broken, NOW.parse().unwrap())), "Key");
        broken.private_key = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
            STANDARD.encode(b"not a key")
        );
        assert_eq!(outcome(assertion(&broken, NOW.parse().unwrap())), "Key");
    }

    async fn token_server(token_calls: u64) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains(
                "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer",
            ))
            .and(body_string_contains("assertion="))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "access-1", "expires_in": 3599, "token_type": "Bearer"
            })))
            .expect(token_calls)
            .mount(&server)
            .await;
        server
    }

    async fn send_to(server: &MockServer, fcm_url: &str) -> String {
        let mut sender = Sender::new(account(&format!("{}/token", server.uri())), fcm_url);
        outcome(
            sender
                .send(NOW.parse().unwrap(), "device-token", &fixture("ended"))
                .await,
        )
    }

    #[tokio::test]
    async fn sends_with_a_token_reused_until_it_expires() {
        let server = token_server(2).await;
        Mock::given(method("POST"))
            .and(path("/v1/projects/demo-project/messages:send"))
            .and(header("authorization", "Bearer access-1"))
            .and(body_json(message("device-token", &fixture("ended"))))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "name": "projects/demo-project/messages/1" }),
                ),
            )
            .expect(3)
            .mount(&server)
            .await;
        let mut sender = Sender::new(account(&format!("{}/token", server.uri())), &server.uri());
        let now: Timestamp = NOW.parse().unwrap();
        for minutes in [0, 30, 59] {
            // The token is renewed a minute before it expires, so the third send asks for a new one.
            let at = now + SignedDuration::from_mins(minutes);
            assert_eq!(
                outcome(sender.send(at, "device-token", &fixture("ended")).await),
                "ok"
            );
        }
    }

    #[tokio::test]
    async fn an_unknown_device_is_reported_as_unregistered() {
        let server = token_server(1).await;
        Mock::given(path("/v1/projects/demo-project/messages:send"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": { "code": 404, "status": "NOT_FOUND", "details": [{ "errorCode": "UNREGISTERED" }] }
            })))
            .mount(&server)
            .await;
        assert_eq!(send_to(&server, &server.uri()).await, "Unregistered");
    }

    #[tokio::test]
    async fn other_failures_keep_their_status() {
        let server = token_server(1).await;
        Mock::given(path("/v1/projects/demo-project/messages:send"))
            .respond_with(ResponseTemplate::new(400).set_body_string("INVALID_ARGUMENT"))
            .mount(&server)
            .await;
        assert_eq!(
            send_to(&server, &server.uri()).await,
            r#"Status(400, "INVALID_ARGUMENT")"#
        );
    }

    #[tokio::test]
    async fn a_refused_token_request_is_reported() {
        let server = MockServer::start().await;
        Mock::given(path("/token"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid_grant"))
            .mount(&server)
            .await;
        assert_eq!(
            send_to(&server, &server.uri()).await,
            r#"Status(401, "invalid_grant")"#
        );
    }

    #[tokio::test]
    async fn an_unreadable_token_response_is_an_http_error() {
        let server = MockServer::start().await;
        Mock::given(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;
        assert_eq!(send_to(&server, &server.uri()).await, "http");
    }

    #[tokio::test]
    async fn unreachable_servers_are_http_errors() {
        // Nothing listens on port 9: first the token server, then Firebase itself.
        let mut sender = Sender::new(account("http://127.0.0.1:9/token"), "http://127.0.0.1:9");
        assert_eq!(
            outcome(
                sender
                    .send(NOW.parse().unwrap(), "t", &fixture("ended"))
                    .await
            ),
            "http"
        );
        let server = token_server(1).await;
        assert_eq!(send_to(&server, "http://127.0.0.1:9").await, "http");
    }

    #[tokio::test]
    async fn a_request_that_hangs_is_an_http_error() {
        // Waiting for ever would stop the poll loop without a word.
        let server = MockServer::start().await;
        Mock::given(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "a", "expires_in": 3599 }))
                    .set_delay(std::time::Duration::from_secs(1)),
            )
            .mount(&server)
            .await;
        let mut sender = Sender::with_timeout(
            account(&format!("{}/token", server.uri())),
            &server.uri(),
            std::time::Duration::from_millis(100),
        );
        assert_eq!(
            outcome(
                sender
                    .send(NOW.parse().unwrap(), "t", &fixture("ended"))
                    .await
            ),
            "http"
        );
    }

    #[tokio::test]
    async fn a_bad_key_stops_the_send() {
        let mut broken = account("http://127.0.0.1:9/token");
        broken.private_key = "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----".into();
        let mut sender = Sender::new(broken, "http://127.0.0.1:9");
        assert_eq!(
            outcome(
                sender
                    .send(NOW.parse().unwrap(), "t", &fixture("ended"))
                    .await
            ),
            "Key"
        );
    }
}
