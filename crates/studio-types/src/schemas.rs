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
    vec![
        ("rfq", rfq()),
        ("rfq-record", rfq_record()),
        ("quote", quote()),
        ("quote-record", quote_record()),
        ("gate-policy", gate_policy()),
        ("project", project()),
        ("field-error", field_error()),
    ]
}

/// Look up one published schema by wire name.
pub fn get(name: &str) -> Option<serde_json::Value> {
    all().into_iter().find(|(n, _)| *n == name).map(|(_, s)| s)
}

/// `schemas/rfq.json` — the RFQ submission contract (`POST /api/v1/rfqs`).
pub fn rfq() -> serde_json::Value {
    finalize("rfq", schema_for!(crate::rfq::NewRfq))
}

/// `schemas/quote.json` — the quote submission contract
/// (`POST /api/v1/rfqs/{id}/quote`). Self-contained: the gate policy is
/// inlined under `$defs` rather than `$ref`'d across files.
pub fn quote() -> serde_json::Value {
    finalize("quote", schema_for!(crate::quote::NewQuote))
}

/// `schemas/rfq-record.json` — the captured demand record, as returned by
/// `POST /api/v1/rfqs` and the RFQ reads (submission + server-assigned
/// `id` / `created_at`).
pub fn rfq_record() -> serde_json::Value {
    finalize("rfq-record", schema_for!(crate::rfq::Rfq))
}

/// `schemas/quote-record.json` — the issued quote, as returned by the quote
/// endpoints (submission + identity, `policy_hash`, status, lifecycle
/// timestamps).
pub fn quote_record() -> serde_json::Value {
    finalize("quote-record", schema_for!(crate::quote::Quote))
}

/// `schemas/field-error.json` — one field-level validation failure; 422
/// bodies are `{ "errors": [field-error, …] }`.
pub fn field_error() -> serde_json::Value {
    finalize("field-error", schema_for!(crate::rfq::FieldError))
}

/// `schemas/gate-policy.json` — the standalone gate-policy contract, for
/// consumers that exchange policies outside a quote.
pub fn gate_policy() -> serde_json::Value {
    finalize("gate-policy", schema_for!(crate::gate::GatePolicy))
}

/// `schemas/project.json` — the public project view
/// (`GET /api/v1/projects/{id}`), rendered by the embedded `/project/{id}`
/// page. Read-only contract: deliberately carries no commercial fields.
pub fn project() -> serde_json::Value {
    finalize("project", schema_for!(crate::project::Project))
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
