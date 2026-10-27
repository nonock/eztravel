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
use serde::{Deserialize, Deserializer, Serialize};

use crate::alerts::{Notification, shown};
use crate::disruptions::{Disruption, Severity};
use crate::http;
use crate::relevance::Scope;
use crate::trains::{Train, Why};

/// Firebase caps the data of a message at 4 KB: titles and texts are cut well below.
const TITLE_MAX_BYTES: usize = 300;
const TEXT_MAX_BYTES: usize = 3000;
/// An alert older than this is no longer worth showing.
const TIME_TO_LIVE: &str = "1800s";
/// Sent in the evening, the next day's trouble waits until the morning for an offline phone.
const EVE_TIME_TO_LIVE: &str = "43200s";
/// A train's message stays this long after the train was due, late or not.
const TRAIN_SHOWN_AFTER: SignedDuration = SignedDuration::from_mins(10);
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
    /// Firebase does not know the device: the app was uninstalled, its token
    /// renewed, or the token is not one.
    Unregistered,
    /// Firebase refuses the message itself: sent again, it would be refused again.
    Refused(String),
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
        let (name, mode, _) = label(code, labels);
        put("line", &name);
        put("mode", mode);
    };
    match notification {
        Notification::Started {
            line: code,
            severity,
            disruptions,
            scope,
        } => {
            put("type", "started");
            line(code, &mut put);
            put("until", &until);
            describe(*severity, disruptions, scope, &mut put);
        }
        Notification::Changed {
            line: code,
            from,
            to,
            disruptions,
            scope,
        } => {
            put("type", "changed");
            line(code, &mut put);
            put("until", &until);
            put("previous", severity_name(*from));
            describe(*to, disruptions, scope, &mut put);
        }
        Notification::Ended { line: code } => {
            put("type", "ended");
            line(code, &mut put);
            put("until", &until);
        }
        Notification::Planned { disruptions } => {
            put("type", "planned");
            titled(disruptions, &mut put);
        }
        Notification::Tomorrow { date, disruptions } => {
            put("type", "tomorrow");
            put("date", &date.to_string());
            put("until", &until);
            titled(disruptions, &mut put);
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
        Notification::Leave {
            train,
            at,
            platform,
            live,
        } => {
            put("type", "leave");
            describe_train(train, labels, &mut put);
            put("leave", &at.to_string());
            if let Some(platform) = platform {
                put("platform", platform);
            }
            put("live", if *live { "true" } else { "false" });
        }
        Notification::Late { train } => {
            put("type", "train_late");
            describe_train(train, labels, &mut put);
        }
    }
    data
}

/// How many records, and their titles, one a line.
fn titled(disruptions: &[Disruption], put: &mut dyn FnMut(&str, &str)) {
    put("count", &disruptions.len().to_string());
    let titles: Vec<&str> = disruptions
        .iter()
        .map(|disruption| disruption.title.as_str())
        .collect();
    put("titles", &truncate(&titles.join("\n"), TEXT_MAX_BYTES));
}

fn describe_train(train: &Train, labels: &[LineLabel], put: &mut dyn FnMut(&str, &str)) {
    let (name, mode, _) = label(&train.line, labels);
    put("line", &name);
    put("mode", mode);
    if let Some(mission) = &train.mission {
        put("mission", mission);
    }
    put("destination", &train.destination);
    // Not `from`, which Firebase keeps for itself.
    put("station", &train.from);
    put("aimed", &train.aimed.to_string());
    put("expected", &train.expected.to_string());
    let until = train.aimed.max(train.expected) + TRAIN_SHOWN_AFTER;
    put("until", &until.to_string());
}

/// How the line `code` is named on screen, its mode (`train`, `metro`, or
/// `line` when unlabelled) and its color on the home screen, if configured.
pub fn label(code: &str, labels: &[LineLabel]) -> (String, &'static str, Option<String>) {
    match labels.iter().find(|label| label.code == code) {
        Some(label) => (
            label.name.clone(),
            if label.mode == Mode::Train {
                "train"
            } else {
                "metro"
            },
            label.color.clone(),
        ),
        None => (code.to_string(), "line", None),
    }
}

/// Severity, where it is, and the record shown: its id and words.
fn describe(
    severity: Severity,
    disruptions: &[Disruption],
    scope: &Scope,
    put: &mut dyn FnMut(&str, &str),
) {
    put("severity", severity_name(severity));
    match scope {
        Scope::Line => put("scope", "line"),
        Scope::Section { from, to } => {
            put("scope", "section");
            put("section_from", &truncate(from, TITLE_MAX_BYTES));
            put("section_to", &truncate(to, TITLE_MAX_BYTES));
        }
        Scope::Stops { first, count } => {
            put("scope", "stops");
            put("stop", &truncate(first, TITLE_MAX_BYTES));
            put("stop_count", &count.to_string());
        }
    }
    if let Some(record) = shown(severity, disruptions) {
        put("id", &record.id);
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
    let kind = data.get("type").map(String::as_str);
    let collapse_key = match (data.get("line"), data.get("aimed"), kind) {
        // A train's own: it must not replace its line's state, nor another train.
        (Some(line), Some(aimed), _) => format!("train-{line}-{aimed}"),
        (Some(line), None, _) => format!("line-{line}"),
        (None, _, Some("planned")) => "planned".to_string(),
        (None, _, Some("tomorrow")) => "tomorrow".to_string(),
        // An outage and its end.
        _ => "watch".to_string(),
    };
    let ttl = if kind == Some("tomorrow") {
        EVE_TIME_TO_LIVE
    } else {
        TIME_TO_LIVE
    };
    serde_json::json!({
        "message": {
            "token": device,
            "data": data,
            "android": { "priority": "HIGH", "ttl": ttl, "collapse_key": collapse_key },
        }
    })
}

/// A web link of an operator's message, on the words of its text it covers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Link {
    /// Its first character and the one after its last, counted in Unicode
    /// characters (code points) from the start of the text.
    pub start: usize,
    pub end: usize,
    /// Always a secure web address: `https://` and a domain name.
    pub url: String,
}

/// Operator messages are HTML: keep the text, one line per paragraph.
pub fn plain_text(html: &str) -> String {
    text_with_links(html).0
}

/// The text of an operator's message, as [`plain_text`], and its links: its
/// anchors to secure web addresses, and such addresses written out in the
/// text. Any other link stays text only.
pub fn text_with_links(html: &str) -> (String, Vec<Link>) {
    // Each character, with the link it is in.
    let mut marked: Vec<(char, Option<usize>)> = Vec::new();
    let mut urls: Vec<String> = Vec::new();
    let mut link: Option<usize> = None;
    let mut words = String::new();
    let mut tag: Option<String> = None;
    for character in html.chars() {
        match (&mut tag, character) {
            (None, '<') => {
                // No entity holds a tag: the words before it are decoded.
                marked.extend(decode_entities(&words).chars().map(|c| (c, link)));
                words.clear();
                tag = Some(String::new());
            }
            (Some(name), '>') => {
                let name = name.trim();
                let lower = name.to_lowercase();
                if ["br", "/p", "/li", "/div"]
                    .iter()
                    .any(|prefix| lower.starts_with(prefix))
                {
                    marked.push(('\n', None));
                }
                match lower.split(char::is_whitespace).next() {
                    Some("a") => {
                        link = href(name).and_then(web_address).map(|url| {
                            urls.push(url);
                            urls.len() - 1
                        });
                    }
                    Some("/a") => link = None,
                    _ => {}
                }
                tag = None;
            }
            (Some(name), _) => name.push(character),
            // In HTML a line break in the source is just a space.
            (None, '\n' | '\r' | '\t') => words.push(' '),
            (None, _) => words.push(character),
        }
    }
    marked.extend(decode_entities(&words).chars().map(|c| (c, link)));
    let marked = lines(&marked);
    let text = marked.iter().map(|&(character, _)| character).collect();
    let mut links = Vec::new();
    let mut at = 0;
    while at < marked.len() {
        let found = match marked[at].1 {
            Some(link) => {
                let end = marked[at..]
                    .iter()
                    .position(|&(_, other)| other != Some(link))
                    .map_or(marked.len(), |length| at + length);
                Some((end, urls[link].clone()))
            }
            None => written_address(&marked, at),
        };
        match found {
            Some((end, url)) => {
                links.push(Link {
                    start: at,
                    end,
                    url,
                });
                at = end;
            }
            None => at += 1,
        }
    }
    (text, links)
}

/// One line per paragraph, without empty ones, and each run of spaces as one:
/// in a link only when between two of its words.
fn lines(marked: &[(char, Option<usize>)]) -> Vec<(char, Option<usize>)> {
    let mut kept = Vec::new();
    for line in marked.split(|&(character, _)| character == '\n') {
        let mut words: Vec<(char, Option<usize>)> = Vec::new();
        let mut space = false;
        for &(character, link) in line {
            if character.is_whitespace() {
                space = true;
                continue;
            }
            if let Some(&(_, before)) = words.last()
                && space
            {
                words.push((' ', if before == link { link } else { None }));
            }
            space = false;
            words.push((character, link));
        }
        if words.is_empty() {
            continue;
        }
        if !kept.is_empty() {
            kept.push(('\n', None));
        }
        kept.extend(words);
    }
    kept
}

/// The address an anchor points to, quoted or not: `a href="…"`.
fn href(tag: &str) -> Option<String> {
    // The same bytes as the tag: ASCII only changes case.
    let lower = tag.to_ascii_lowercase();
    let (at, _) = lower
        .match_indices("href")
        .find(|&(at, _)| lower[..at].ends_with(char::is_whitespace))?;
    let value = tag[at + 4..].trim_start().strip_prefix('=')?.trim_start();
    let value = match value.chars().next()? {
        quote @ ('"' | '\'') => value[1..].split(quote).next().unwrap_or_default(),
        _ => value.split(char::is_whitespace).next().unwrap_or_default(),
    };
    Some(decode_entities(value.trim()))
}

/// Only a secure web address with a domain name makes a link: `http`,
/// `javascript:` or broken addresses stay text.
fn web_address(address: String) -> Option<String> {
    let host = address
        .strip_prefix("https://")?
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let whole = !address.chars().any(|c| c.is_whitespace() || c.is_control());
    (host.contains('.') && whole).then_some(address)
}

/// A secure web address written out in the text from `start`, outside any
/// link: where it ends, and itself, without the sentence's punctuation after it.
fn written_address(marked: &[(char, Option<usize>)], start: usize) -> Option<(usize, String)> {
    let rest = &marked[start..];
    let begins = rest
        .iter()
        .map(|&(character, _)| character)
        .take(8)
        .eq("https://".chars());
    if !begins || start > 0 && marked[start - 1].0.is_alphanumeric() {
        return None;
    }
    let mut address: String = rest
        .iter()
        .take_while(|&&(character, link)| !character.is_whitespace() && link.is_none())
        .map(|&(character, _)| character)
        .collect();
    while address.ends_with(|c: char| ".,;:!?)]}»\"'’…".contains(c)) {
        address.pop();
    }
    let end = start + address.chars().count();
    web_address(address).map(|url| (end, url))
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
        if status.as_u16() == 400 {
            // The same answer for a token that is not one, and for a message never to be taken.
            return Err(if faults_token(&body) {
                PushError::Unregistered
            } else {
                PushError::Refused(body)
            });
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

/// Whether Firebase's answer faults the device token rather than the message.
fn faults_token(body: &str) -> bool {
    let Ok(answer) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    answer["error"]["details"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|detail| detail["fieldViolations"].as_array())
        .flatten()
        .any(|violation| violation["field"] == "message.token")
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
    fn no_payload_uses_a_key_firebase_refuses() {
        // Firebase answers 400 to these: the notification never leaves.
        let refused = |key: &str| {
            ["from", "message_type"].contains(&key)
                || key.starts_with("google.")
                || key.starts_with("gcm.")
        };
        let folder = format!("{}/../fixtures/push", env!("CARGO_MANIFEST_DIR"));
        for file in std::fs::read_dir(folder).unwrap() {
            let name = file.unwrap().file_name().into_string().unwrap();
            let Some(kind) = name.strip_suffix(".json") else {
                continue;
            };
            let keys: Vec<String> = fixture(kind)
                .into_keys()
                .filter(|key| refused(key))
                .collect();
            assert_eq!(keys, Vec::<String>::new(), "{name}");
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
                Disruption {
                    id: "0c1d7a2e-4f1b-4b55-9a37-2f6c1e0d5b11".into(),
                    ..record(
                        "Ligne L : Nanterre <-> Cergy trafic interrompu",
                        Severity::Blocking,
                        "<p>Le trafic est interrompu entre Nanterre et Cergy jusqu&#39;à 20h.</p><p>Motif : panne d'un train.</p>",
                    )
                },
            ],
            scope: Scope::Section {
                from: "Nanterre".into(),
                to: "Cergy".into(),
            },
        };
        assert_eq!(data(&notification, &labels(), until()), fixture("started"));
    }

    #[test]
    fn a_change_gives_both_severities() {
        let notification = Notification::Changed {
            line: "C02000".into(),
            from: Severity::Blocking,
            to: Severity::Disrupted,
            disruptions: vec![Disruption {
                id: "7f3b9d10-2c44-4e8a-b6a1-5d0e9c2f4a63".into(),
                ..record(
                    "Métro X : Reprise progressive / trafic reste perturbé",
                    Severity::Unknown,
                    "Le trafic reprend mais reste perturbé sur toute la ligne.",
                )
            }],
            scope: Scope::Line,
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
    fn the_next_days_disruptions_are_listed_by_title_with_the_day() {
        let notification = Notification::Tomorrow {
            date: jiff::civil::date(2026, 11, 4),
            disruptions: vec![
                record("Ligne L : travaux", Severity::Disrupted, ""),
                record("Métro X : grève", Severity::Blocking, ""),
            ],
        };
        assert_eq!(data(&notification, &labels(), until()), fixture("tomorrow"));
        // Sent in the evening, it waits for an offline phone until the morning.
        let body = message("t", &fixture("tomorrow"));
        assert_eq!(body["message"]["android"]["collapse_key"], "tomorrow");
        assert_eq!(body["message"]["android"]["ttl"], "43200s");
    }

    #[test]
    fn a_notification_without_record_has_no_text() {
        let started = Notification::Started {
            line: "C01740".into(),
            severity: Severity::Disrupted,
            disruptions: vec![],
            scope: Scope::Line,
        };
        let sent = data(&started, &labels(), until());
        assert_eq!(sent["severity"], "disrupted");
        assert!(
            !sent.contains_key("title") && !sent.contains_key("text") && !sent.contains_key("id")
        );
    }

    #[test]
    fn a_disruption_at_stops_names_the_first_and_counts_them() {
        let started = Notification::Started {
            line: "C02000".into(),
            severity: Severity::Blocking,
            disruptions: vec![record("Métro X : station fermée", Severity::Blocking, "")],
            scope: Scope::Stops {
                first: "Gare Centrale".into(),
                count: 2,
            },
        };
        let sent = data(&started, &labels(), until());
        assert_eq!(
            [&sent["scope"], &sent["stop"], &sent["stop_count"]],
            ["stops", "Gare Centrale", "2"]
        );
        assert!(!sent.contains_key("section_from"));
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
            scope: Scope::Section {
                from: long.clone(),
                to: long.clone(),
            },
        };
        let sent = data(&notification, &labels(), until());
        assert!(sent["section_from"].len() <= TITLE_MAX_BYTES);
        assert!(sent["section_to"].len() <= TITLE_MAX_BYTES);
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

    fn link(start: usize, end: usize, url: &str) -> Link {
        Link {
            start,
            end,
            url: url.into(),
        }
    }

    #[test]
    fn a_message_keeps_its_web_links_on_their_words() {
        let (text, links) = text_with_links(
            "<p>Suivez le <a href=\"https://example.org/ligne-l\">fil de la ligne L</a> et le <a target=\"_blank\" HREF='https://example.org/blog?a=1&amp;b=2'>blog</a>.</p>",
        );
        assert_eq!(text, "Suivez le fil de la ligne L et le blog.");
        assert_eq!(
            links,
            [
                link(10, 27, "https://example.org/ligne-l"),
                link(34, 38, "https://example.org/blog?a=1&b=2"),
            ]
        );
        // The text is the same as without links.
        assert_eq!(
            plain_text("<a href=\"https://example.org/\">blog</a>"),
            "blog"
        );
        // An address need not be quoted.
        let (_, links) = text_with_links("<a href=https://example.org/c target=_blank>c</a>");
        assert_eq!(links, [link(0, 1, "https://example.org/c")]);
    }

    #[test]
    fn a_link_is_placed_by_characters_on_its_own_word() {
        // "➡️" is two characters; the first "ici" is not the link.
        let (text, links) = text_with_links(
            "&#x27A1;&#xFE0F; D&#233;tails <a href=\"https://example.org/d\">ici</a>, pas ici.",
        );
        assert_eq!(text, "➡️ Détails ici, pas ici.");
        assert_eq!(links, [link(11, 14, "https://example.org/d")]);
    }

    #[test]
    fn only_secure_web_addresses_become_links() {
        let (text, links) = text_with_links(concat!(
            "<a href=\"http://example.org/\">site</a> <a href=\"http://iOS\">iOS</a> ",
            "<a href=\"https://Google Store\">Google Store</a> <a href=\"javascript:alert(1)\">x</a> ",
            "<a href=\"https://localhost/\">local</a> <a name=\"haut\">haut</a> <a href=>vide</a> <a href>nu</a>",
        ));
        assert_eq!(text, "site iOS Google Store x local haut vide nu");
        assert_eq!(links, []);
    }

    #[test]
    fn addresses_written_in_the_text_become_links() {
        let (text, links) = text_with_links(
            "Infos : https://example.org/a. Et (https://example.org/b), pas http://example.org/c ni https://localhost/x.",
        );
        assert_eq!(
            links,
            [
                link(8, 29, "https://example.org/a"),
                link(35, 56, "https://example.org/b"),
            ]
        );
        assert_eq!(text.chars().count(), 107);
    }

    #[test]
    fn a_link_holds_its_words_only_once() {
        // Written as its own address: one link, not two.
        let (text, links) = text_with_links(
            "Voir <a href=\"https://example.org/\">https://example.org/</a> ou le <a href=\"https://example.org/b\"> blog </a> .",
        );
        assert_eq!(text, "Voir https://example.org/ ou le blog .");
        assert_eq!(
            links,
            [
                link(5, 25, "https://example.org/"),
                link(32, 36, "https://example.org/b"),
            ]
        );
        // A space is the link's only between two of its words.
        let (text, links) = text_with_links(
            "voir<a href=\"https://example.org/i\"> ici </a>et <a href=\"https://example.org/j\">là</a> <a href=\"https://example.org/k\">ou là</a>",
        );
        assert_eq!(text, "voir ici et là ou là");
        assert_eq!(
            links,
            [
                link(5, 8, "https://example.org/i"),
                link(12, 14, "https://example.org/j"),
                link(15, 20, "https://example.org/k"),
            ]
        );
        // Over a line break: one link on each line.
        let (text, links) = text_with_links("<a href=\"https://example.org/x\">un<br>deux</a>");
        assert_eq!(text, "un\ndeux");
        assert_eq!(
            links,
            [
                link(0, 2, "https://example.org/x"),
                link(3, 7, "https://example.org/x"),
            ]
        );
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
            expected: "2026-11-03T06:52:00Z".parse().unwrap(),
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
        // A late train stays on the phone until after it was expected.
        let late = Notification::TrainCancelled {
            train: Train {
                expected: "2026-11-03T07:05:00Z".parse().unwrap(),
                ..train(Some("PALO"))
            },
            why: Why::Announced,
        };
        assert_eq!(
            data(&late, &labels(), until())["until"],
            "2026-11-03T07:15:00Z"
        );
        assert_eq!(
            data(&late, &labels(), until())["aimed"],
            "2026-11-03T06:52:00Z"
        );
    }

    #[test]
    fn the_reminder_says_when_to_leave_and_how_the_train_runs() {
        let at = |time: &str| -> Timestamp { format!("2026-11-03T{time}:00Z").parse().unwrap() };
        let leave = |platform: Option<&str>, live| Notification::Leave {
            train: Train {
                expected: at("06:54"),
                ..train(Some("PALO"))
            },
            at: at("06:42"),
            platform: platform.map(String::from),
            live,
        };
        assert_eq!(
            data(&leave(Some("2"), true), &labels(), until()),
            fixture("leave")
        );
        let unknown = data(&leave(None, false), &labels(), until());
        assert_eq!(unknown["live"], "false");
        assert!(!unknown.contains_key("platform"));
        let late = Notification::Late {
            train: Train {
                expected: at("06:56"),
                ..train(Some("PALO"))
            },
        };
        assert_eq!(data(&late, &labels(), until()), fixture("train_late"));
    }

    #[test]
    fn a_train_back_replaces_its_cancellation_only() {
        // And a reminder or a delay replaces what was said of the same train.
        for name in ["train_cancelled", "train_back", "leave", "train_late"] {
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
            .respond_with(ResponseTemplate::new(500).set_body_string("INTERNAL"))
            .mount(&server)
            .await;
        assert_eq!(
            send_to(&server, &server.uri()).await,
            r#"Status(500, "INTERNAL")"#
        );
    }

    /// Firebase's answer to a request it will never take, faulting `field`.
    fn invalid(field: &str) -> String {
        serde_json::json!({ "error": {
            "code": 400,
            "status": "INVALID_ARGUMENT",
            "details": [
                { "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": "INVALID_ARGUMENT" },
                { "@type": "type.googleapis.com/google.rpc.BadRequest",
                  "fieldViolations": [{ "field": field, "description": "invalid" }] }
            ]
        } })
        .to_string()
    }

    async fn answered_400(body: String) -> String {
        let server = token_server(1).await;
        Mock::given(path("/v1/projects/demo-project/messages:send"))
            .respond_with(ResponseTemplate::new(400).set_body_string(body))
            .mount(&server)
            .await;
        send_to(&server, &server.uri()).await
    }

    #[tokio::test]
    async fn a_message_firebase_will_never_take_is_refused() {
        let body = invalid("message.data");
        assert_eq!(
            answered_400(body.clone()).await,
            format!("Refused({body:?})")
        );
    }

    #[tokio::test]
    async fn a_token_that_is_not_one_is_a_device_firebase_does_not_know() {
        assert_eq!(answered_400(invalid("message.token")).await, "Unregistered");
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
