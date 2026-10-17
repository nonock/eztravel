//! Sends one example notification to a phone, to try the whole chain by hand:
//!
//!     cargo run --example send_test -- <service account key file> <device token file> [started|changed|ended|planned]
//!
//! The payload is taken from `fixtures/push/`. Both files stay out of the repository.

use std::collections::BTreeMap;

use eztravel_backend::push::{Sender, ServiceAccount};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [key_file, token_file, rest @ ..] = args.as_slice() else {
        eprintln!(
            "usage: send_test <service account key file> <device token file> [started|changed|ended|planned]"
        );
        std::process::exit(2);
    };
    let kind = rest.first().map_or("started", String::as_str);
    let account: ServiceAccount =
        serde_json::from_str(&std::fs::read_to_string(key_file).expect("key file"))
            .expect("service account key");
    let device = std::fs::read_to_string(token_file).expect("device token file");
    let fixture = format!(
        "{}/../fixtures/push/{kind}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let data: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(fixture).expect("payload")).expect("payload");
    let mut sender = Sender::new(account, "https://fcm.googleapis.com");
    match sender
        .send(jiff::Timestamp::now(), device.trim(), &data)
        .await
    {
        Ok(()) => println!("sent: {kind}"),
        Err(error) => {
            eprintln!("not sent: {error:?}");
            std::process::exit(1);
        }
    }
}
