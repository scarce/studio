//! The schema registry — JSON Schemas *generated* from the wire types.
//!
//! `schemas/*.json` in the repo root are build artifacts of this module
//! (regenerate with `just schemas`); a drift test asserts the checked-in
//! files equal the generated output, so the interoperable contract
//! (DESIGN.md §8) is derived from the Rust types instead of hand-maintained.
//! The API serves the same values at `GET /api/v1/schemas/{name}`.

use schemars::schema_for;

/// Stable `$id` base for published schemas.
const ID_BASE: &str = "https://scarce.studio/schemas";

/// Every published schema, by wire name (= filename stem under `schemas/`).
pub fn all() -> Vec<(&'static str, serde_json::Value)> {
    vec![("rfq", rfq())]
}

/// Look up one published schema by wire name.
pub fn get(name: &str) -> Option<serde_json::Value> {
    all().into_iter().find(|(n, _)| *n == name).map(|(_, s)| s)
}

/// `schemas/rfq.json` — the RFQ submission contract (`POST /api/v1/rfqs`).
pub fn rfq() -> serde_json::Value {
    finalize("rfq", schema_for!(crate::rfq::NewRfq))
}

/// Stamp the registry-level `$id` onto a generated schema. `$schema`, title,
/// and descriptions come from the type derives.
fn finalize(name: &str, schema: schemars::Schema) -> serde_json::Value {
    let mut value = serde_json::to_value(&schema).expect("schema serializes");
    value
        .as_object_mut()
        .expect("schema is an object")
        .insert("$id".into(), format!("{ID_BASE}/{name}.json").into());
    value
}
