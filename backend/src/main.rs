//! The eztravel backend: reads PRIM every two minutes during the watch
//! windows, notifies the phone, and serves the API the app calls.
//! Configuration comes from environment variables, listed on `Service::from_env`.

use std::time::Duration;

use eztravel_backend::service::{Poll, Service, Startup};
use jiff::Timestamp;

/// PRIM rebuilds its disruption feed about every two minutes.
const PERIOD_SECONDS: i64 = 120;

#[tokio::main]
async fn main() {
    let Startup { mut service, api } = Service::from_env(|name| std::env::var(name).ok())
        .unwrap_or_else(|error| {
            eprintln!("cannot start: {error}");
            std::process::exit(1);
        });
    // All interfaces: fly.io's proxy cannot reach a loopback listener.
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap_or_else(|error| {
            eprintln!("cannot listen on port 8080: {error}");
            std::process::exit(1);
        });
    println!(
        "eztravel backend started, {}",
        service.describe(Timestamp::now())
    );
    // If either half stops, the process ends and fly.io restarts both.
    tokio::select! {
        result = axum::serve(listener, api) => eprintln!("the API stopped: {result:?}"),
        () = poll_forever(&mut service) => {}
    }
    std::process::exit(1);
}

async fn poll_forever(service: &mut Service) {
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
