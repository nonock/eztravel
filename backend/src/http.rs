//! The HTTP client behind every outgoing call, to PRIM and to Firebase.

use std::time::Duration;

/// A call that hangs would stop the poll loop without a word: none takes longer than this.
pub const TIMEOUT: Duration = Duration::from_secs(30);

pub fn client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .expect("the TLS backend loads")
}
