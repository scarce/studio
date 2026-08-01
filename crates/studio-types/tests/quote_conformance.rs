//! Keeps `schemas/quote.json` + `schemas/gate-policy.json` (the interoperable
//! wire contract, DESIGN.md §8) and the Rust validation honest with each
//! other, exactly like the RFQ suite: shared payloads must be accepted or
//! rejected by *both* validators.
//!
//! One deliberate asymmetry, stated in both schemas' descriptions: the code
//! enforces relational rules JSON Schema cannot express (milestone amounts
//! sum to the price, split bps sum to 10000, k ≤ n, no self-loop edges,
//! expiry in the future). For those, `code_stricter` asserts the divergence
//! explicitly — schema-valid, code-invalid — so the boundary is pinned by
//! tests rather than left to drift.

use chrono::{DateTime, Utc};
use studio_types::{GatePolicy, NewQuote};

const QUOTE_SCHEMA: &str = include_str!("../../../schemas/quote.json");
const GATE_POLICY_SCHEMA: &str = include_str!("../../../schemas/gate-policy.json");

const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
/// Validation is anchored at issue time; expiry vectors are relative to this.
const NOW: &str = "2026-08-01T15:00:00Z";

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(NOW)
        .unwrap()
        .with_timezone(&Utc)
}

/// The generated quote schema is self-contained (the gate policy is inlined
/// under `$defs`), so no cross-file resource registration is needed.
fn quote_validator() -> jsonschema::Validator {
    let quote: serde_json::Value =
        serde_json::from_str(QUOTE_SCHEMA).expect("schemas/quote.json parses");
    jsonschema::validator_for(&quote).expect("schemas/quote.json compiles")
}

fn gate_policy_validator() -> jsonschema::Validator {
    let gate_policy: serde_json::Value =
        serde_json::from_str(GATE_POLICY_SCHEMA).expect("schemas/gate-policy.json parses");
    jsonschema::validator_for(&gate_policy).expect("schemas/gate-policy.json compiles")
}

fn quote_code_verdict(payload: &serde_json::Value) -> bool {
    serde_json::from_value::<NewQuote>(payload.clone())
        .ok()
        .is_some_and(|q| q.validate(now()).is_ok())
}

fn agree_quote(payload: serde_json::Value, expect_valid: bool) {
    assert_eq!(
        quote_validator().is_valid(&payload),
        expect_valid,
        "schema verdict diverged for {payload}"
    );
    assert_eq!(
        quote_code_verdict(&payload),
        expect_valid,
        "code verdict diverged for {payload}"
    );
}

/// The documented one-way divergence: schema accepts, code rejects.
fn code_stricter_quote(payload: serde_json::Value) {
    assert!(
        quote_validator().is_valid(&payload),
        "expected schema-valid for {payload}"
    );
    assert!(
        !quote_code_verdict(&payload),
        "expected code-invalid for {payload}"
    );
}

fn agree_policy(payload: serde_json::Value, expect_valid: bool) {
    let code_verdict = serde_json::from_value::<GatePolicy>(payload.clone())
        .ok()
        .is_some_and(|p| p.validate("gate_policy").is_ok());
    assert_eq!(
        gate_policy_validator().is_valid(&payload),
        expect_valid,
        "schema verdict diverged for {payload}"
    );
    assert_eq!(
        code_verdict, expect_valid,
        "code verdict diverged for {payload}"
    );
}

fn valid_quote() -> serde_json::Value {
    serde_json::json!({
        "price": { "amount": 250_000_000, "mint": USDC },
        "milestones": [
            { "title": "Forecast model", "description": "p50/p90 per program id", "amount": 150_000_000 },
            { "title": "Gated endpoint", "description": "pay.sh-gated REST endpoint", "amount": 100_000_000 }
        ],
        "timeline": "2 weeks, weekly demos",
        "payout_destination": { "kind": "splits", "splits": [
            { "recipient": "CrewAgentA111111111111111111111111111111111", "bps": 7000 },
            { "recipient": "CrewAgentB111111111111111111111111111111111", "bps": 3000 }
        ]},
        "channel": { "grace_seconds": 172_800, "idle_timeout_seconds": 604_800 },
        "expires_at": "2026-08-08T15:00:00Z"
    })
}

fn with(
    mut payload: serde_json::Value,
    patch: impl FnOnce(&mut serde_json::Value),
) -> serde_json::Value {
    patch(&mut payload);
    payload
}

// ── agreement: quote ─────────────────────────────────────────────────────

#[test]
fn minimal_quote_without_gate_policy_is_valid_in_both() {
    // gate_policy omitted → studio default; grace_seconds omitted → 48h.
    agree_quote(
        with(valid_quote(), |q| {
            q["channel"] = serde_json::json!({ "idle_timeout_seconds": 3600 });
        }),
        true,
    );
}

#[test]
fn quote_with_explicit_default_gate_policy_is_valid_in_both() {
    agree_quote(
        with(valid_quote(), |q| {
            q["gate_policy"] = serde_json::to_value(GatePolicy::studio_default()).unwrap();
        }),
        true,
    );
}

#[test]
fn rejects_agree_empty_milestones() {
    agree_quote(
        with(valid_quote(), |q| q["milestones"] = serde_json::json!([])),
        false,
    );
}

#[test]
fn rejects_agree_zero_milestone_amount() {
    agree_quote(
        with(valid_quote(), |q| {
            q["milestones"][0]["amount"] = serde_json::json!(0)
        }),
        false,
    );
}

#[test]
fn rejects_agree_missing_channel() {
    agree_quote(
        with(valid_quote(), |q| {
            q.as_object_mut().unwrap().remove("channel");
        }),
        false,
    );
}

#[test]
fn rejects_agree_unknown_payout_kind() {
    agree_quote(
        with(valid_quote(), |q| {
            q["payout_destination"] = serde_json::json!({ "kind": "vault", "address": "x" })
        }),
        false,
    );
}

#[test]
fn rejects_agree_unknown_top_level_field() {
    agree_quote(
        with(valid_quote(), |q| q["surprise"] = serde_json::json!(true)),
        false,
    );
}

#[test]
fn rejects_agree_bad_gate_spec_in_policy() {
    agree_quote(
        with(valid_quote(), |q| {
            q["gate_policy"] = serde_json::json!({ "edges": {
                "QUOTED->FUNDED": [ { "type": "timelock" } ]   // min_seconds missing
            }});
        }),
        false,
    );
}

// ── code stricter than schema (documented, pinned) ───────────────────────

#[test]
fn code_stricter_milestone_sum_must_equal_price() {
    code_stricter_quote(with(valid_quote(), |q| {
        q["milestones"][1]["amount"] = serde_json::json!(99_000_000)
    }));
}

#[test]
fn code_stricter_split_bps_must_sum_to_10000() {
    code_stricter_quote(with(valid_quote(), |q| {
        q["payout_destination"]["splits"][1]["bps"] = serde_json::json!(2999)
    }));
}

#[test]
fn code_stricter_expiry_must_be_in_the_future() {
    code_stricter_quote(with(valid_quote(), |q| {
        q["expires_at"] = serde_json::json!("2026-08-01T14:00:00Z")
    }));
}

#[test]
fn code_stricter_self_loop_edge_is_rejected() {
    code_stricter_quote(with(valid_quote(), |q| {
        q["gate_policy"] = serde_json::json!({ "edges": {
            "QUOTED->QUOTED": [ { "type": "payment_evidence" } ]
        }});
    }));
}

#[test]
fn code_stricter_k_cannot_exceed_named_agents() {
    code_stricter_quote(with(valid_quote(), |q| {
        q["gate_policy"] = serde_json::json!({ "edges": {
            "DEMOED->ACCEPTED": [ { "type": "agent_signoff", "agents": ["reviewer"], "k": 2 } ]
        }});
    }));
}

// ── agreement: gate-policy standalone ────────────────────────────────────

#[test]
fn default_policy_is_valid_in_both() {
    agree_policy(
        serde_json::to_value(GatePolicy::studio_default()).unwrap(),
        true,
    );
}

#[test]
fn rejects_agree_unknown_edge_key() {
    agree_policy(
        serde_json::json!({ "edges": { "NOPE->FUNDED": [ { "type": "payment_evidence" } ] } }),
        false,
    );
}

#[test]
fn rejects_agree_unknown_gate_type() {
    agree_policy(
        serde_json::json!({ "edges": { "QUOTED->FUNDED": [ { "type": "vibes" } ] } }),
        false,
    );
}

#[test]
fn rejects_agree_cross_variant_field_bleed() {
    agree_policy(
        serde_json::json!({ "edges": { "QUOTED->FUNDED": [
            { "type": "timelock", "min_seconds": 5, "principal": "buyer" }
        ] } }),
        false,
    );
}

#[test]
fn rejects_agree_zero_k() {
    agree_policy(
        serde_json::json!({ "edges": { "DEMOED->ACCEPTED": [
            { "type": "agent_signoff", "agents": ["reviewer"], "k": 0 }
        ] } }),
        false,
    );
}
