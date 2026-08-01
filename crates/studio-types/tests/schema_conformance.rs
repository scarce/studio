//! Keeps `schemas/rfq.json` (the interoperable wire contract, DESIGN.md §8)
//! and `NewRfq::validate` (the enforcement) honest with each other: every
//! example must be accepted or rejected by *both*, or the schema has drifted
//! from the code.

use studio_types::NewRfq;

const RFQ_SCHEMA: &str = include_str!("../../../schemas/rfq.json");

const GOOD_NPUB: &str = "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy";

fn schema() -> serde_json::Value {
    serde_json::from_str(RFQ_SCHEMA).expect("schemas/rfq.json parses")
}

fn agree(payload: serde_json::Value, expect_valid: bool) {
    let schema_verdict = jsonschema::is_valid(&schema(), &payload);
    let code_verdict = serde_json::from_value::<NewRfq>(payload.clone())
        .ok()
        .is_some_and(|rfq| rfq.validate().is_ok());
    assert_eq!(
        schema_verdict, expect_valid,
        "schema verdict diverged for {payload}"
    );
    assert_eq!(
        code_verdict, expect_valid,
        "code verdict diverged for {payload}"
    );
}

#[test]
fn minimal_submission_is_valid_in_both() {
    agree(
        serde_json::json!({ "query": "tls cert chain decoder api", "buyer_npub": GOOD_NPUB }),
        true,
    );
}

#[test]
fn full_submission_is_valid_in_both() {
    agree(
        serde_json::json!({
            "query": "solana priority fee forecast api",
            "product": "REST endpoint forecasting p50/p90 priority fees per program id",
            "monetization": "per-call, usd-denominated",
            "competition": ["triton", "helius fee api"],
            "budget_ceiling": { "amount": 250_000_000, "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" },
            "buyer_npub": GOOD_NPUB
        }),
        true,
    );
}

#[test]
fn rejects_agree_missing_query() {
    agree(serde_json::json!({ "buyer_npub": GOOD_NPUB }), false);
}

#[test]
fn rejects_agree_bad_npub() {
    agree(
        serde_json::json!({ "query": "x", "buyer_npub": "npub1short" }),
        false,
    );
}

#[test]
fn rejects_agree_zero_budget() {
    agree(
        serde_json::json!({
            "query": "x",
            "buyer_npub": GOOD_NPUB,
            "budget_ceiling": { "amount": 0, "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" }
        }),
        false,
    );
}

#[test]
fn rejects_agree_unknown_field() {
    agree(
        serde_json::json!({ "query": "x", "buyer_npub": GOOD_NPUB, "surprise": true }),
        false,
    );
}
