//! RFQ capture — validate a submission and assemble the demand record.
//!
//! Pure: the caller supplies identity and time, so every surface (HTTP
//! handler, CLI, MCP tool) produces bit-identical records and the logic is
//! testable without a clock or an RNG.

use chrono::{DateTime, Utc};
use studio_types::{FieldError, NewRfq, Rfq};

/// Validate `new` and assemble the captured record. The single path from
/// submission to `Rfq` — handlers only supply `id` and `now`.
pub fn capture(new: NewRfq, id: String, now: DateTime<Utc>) -> Result<Rfq, Vec<FieldError>> {
    new.validate()?;
    Ok(Rfq {
        id,
        query: new.query,
        product: new.product,
        monetization: new.monetization,
        competition: new.competition,
        budget_ceiling: new.budget_ceiling,
        buyer_npub: new.buyer_npub,
        buyer_signature: new.buyer_signature,
        created_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD_NPUB: &str = "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy";

    #[test]
    fn capture_assigns_exactly_the_given_identity_and_time() {
        let now = Utc::now();
        let rfq = capture(
            NewRfq {
                query: "tls cert chain decoder api".into(),
                product: None,
                monetization: None,
                competition: vec!["openssl s_client".into()],
                budget_ceiling: None,
                buyer_npub: GOOD_NPUB.into(),
                buyer_signature: None,
            },
            "rfq-1".into(),
            now,
        )
        .unwrap();
        assert_eq!(rfq.id, "rfq-1");
        assert_eq!(rfq.created_at, now);
        assert_eq!(rfq.competition, vec!["openssl s_client".to_string()]);
    }

    #[test]
    fn capture_refuses_invalid_submissions() {
        let errors = capture(
            NewRfq {
                query: "  ".into(),
                product: None,
                monetization: None,
                competition: vec![],
                budget_ceiling: None,
                buyer_npub: GOOD_NPUB.into(),
                buyer_signature: None,
            },
            "rfq-1".into(),
            Utc::now(),
        )
        .unwrap_err();
        assert_eq!(errors[0].field, "query");
    }
}
