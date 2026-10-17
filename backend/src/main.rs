//! The eztravel backend: reads PRIM every two minutes during the watch
//! windows and notifies the phone. Configuration comes from environment
//! variables, listed on `Service::from_env`.

use std::time::Duration;

use eztravel_backend::service::{Poll, Service};
use jiff::Timestamp;

/// PRIM rebuilds its disruption feed about every two minutes.
const PERIOD_SECONDS: i64 = 120;

#[tokio::main]
async fn main() {
    let mut service = Service::from_env(|name| std::env::var(name).ok()).unwrap_or_else(|error| {
        eprintln!("cannot start: {error}");
        std::process::exit(1);
    });
    println!(
        "eztravel backend started, {}",
        service.describe(Timestamp::now())
    );
    loop {
        let now = Timestamp::now();
        match service.poll(now).await {
            Poll::Idle => {}
            outcome => println!("{now}: {outcome:?}"),
        }
        let wait = PERIOD_SECONDS - now.as_second().rem_euclid(PERIOD_SECONDS);
        tokio::time::sleep(Duration::from_secs(wait.unsigned_abs())).await;
    }
}
