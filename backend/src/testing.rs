//! What the tests of several modules share.

/// An answer of the API, as `fixtures/api/` fixes it for both sides.
pub(crate) fn fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/../fixtures/api/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
