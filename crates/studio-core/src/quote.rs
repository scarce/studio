//! Quote issuance — validate a submission and assemble the issued quote.
//!
//! Pure, like `rfq::capture`: the caller supplies identity and time, and
//! `now` anchors both the expiry validation and `created_at`, so every
//! surface (HTTP handler, CLI, MCP tool) issues bit-identical quotes.

use chrono::{DateTime, Utc};
use studio_types::{FieldError, NewQuote, Quote, QuoteStatus};

use crate::gate::commitment_hash;

/// Validate `new` against `now` and assemble the issued quote, including the
/// gate-policy commitment hash (PLAN.md §2.1(4)) and the quote's own
/// commitment hash (capability-request draft-00 §4 — session terms hash-commit
/// the quote at accept). The single path from submission to `Quote` —
/// handlers only supply `rfq_id`, `id`, and `now`.
pub fn issue(
    new: NewQuote,
    rfq_id: String,
    id: String,
    now: DateTime<Utc>,
) -> Result<Quote, Vec<FieldError>> {
    new.validate(now)?;
    Ok(Quote {
        id,
        rfq_id,
        policy_hash: commitment_hash(&new.gate_policy),
        quote_hash: String::new(),
        price: new.price,
        milestones: new.milestones,
        timeline: new.timeline,
        payout_destination: new.payout_destination,
        channel: new.channel,
        engagement_endpoint: new.engagement_endpoint,
        gate_policy: new.gate_policy,
        expires_at: new.expires_at,
        status: QuoteStatus::Quoted,
        created_at: now,
        lapsed_at: None,
        accepted_at: None,
    }
    .with_commitment_hash())
}

/// Why an acceptance was refused. Fail-closed like the gate engine: anything
/// that is not a live QUOTED quote refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptError {
    AlreadyAccepted,
    Lapsed,
}

/// Decide acceptance for `quote` as of `now` — pure; the store's atomic
/// UPDATE guard enforces the same rule against races. ACCEPTED stands in for
/// FUNDED while payments are stubbed (PLAN.md §6 override path).
pub fn accept(quote: &Quote, now: DateTime<Utc>) -> Result<DateTime<Utc>, AcceptError> {
    match quote.clone().at(now).status {
        QuoteStatus::Accepted => Err(AcceptError::AlreadyAccepted),
        QuoteStatus::Lapsed => Err(AcceptError::Lapsed),
        QuoteStatus::Quoted => Ok(now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_types::{
        Amount, ChannelParams, GatePolicy, MilestoneSpec, PayoutDestination, Split,
    };

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    fn valid() -> NewQuote {
        NewQuote {
            price: Amount {
                amount: 100,
                mint: USDC.into(),
            },
            milestones: vec![MilestoneSpec {
                title: "all of it".into(),
                description: "one milestone".into(),
                amount: 100,
            }],
            timeline: "3 days".into(),
            payout_destination: PayoutDestination::Splits {
                splits: vec![Split {
                    recipient: "CrewAgentA111111111111111111111111111111111".into(),
                    bps: 10_000,
                }],
            },
            channel: ChannelParams {
                grace_seconds: 172_800,
                idle_timeout_seconds: 3_600,
            },
            engagement_endpoint: "https://scarce.sh/api/v1/engagements/rfq-1".into(),
            gate_policy: GatePolicy::studio_default(),
            expires_at: ts("2026-08-08T15:00:00Z"),
        }
    }

    #[test]
    fn issue_assigns_identity_time_status_and_both_hashes() {
        let now = ts("2026-08-01T15:00:00Z");
        let quote = issue(valid(), "rfq-1".into(), "q-1".into(), now).unwrap();
        assert_eq!(quote.id, "q-1");
        assert_eq!(quote.rfq_id, "rfq-1");
        assert_eq!(quote.status, QuoteStatus::Quoted);
        assert_eq!(quote.created_at, now);
        assert_eq!(quote.lapsed_at, None);
        assert_eq!(
            quote.policy_hash,
            commitment_hash(&GatePolicy::studio_default())
        );
        // sealed at issue: the recorded hash is the recomputable commitment
        assert_eq!(quote.quote_hash, quote.commitment_hash());
    }

    #[test]
    fn issue_refuses_invalid_submissions_anchored_at_now() {
        // Valid shape, but expired relative to `now` — validation and
        // assembly share the same clock by construction.
        let errors = issue(
            valid(),
            "rfq-1".into(),
            "q-1".into(),
            ts("2026-08-09T00:00:00Z"),
        )
        .unwrap_err();
        assert_eq!(errors[0].field, "expires_at");
    }

    #[test]
    fn accept_only_live_quoted_and_fail_closed_on_expiry() {
        let issued_at = ts("2026-08-01T15:00:00Z");
        let quote = issue(valid(), "rfq-1".into(), "q-1".into(), issued_at).unwrap();

        // live: accepts at `now`
        let now = ts("2026-08-02T00:00:00Z");
        assert_eq!(accept(&quote, now), Ok(now));

        // past expiry: refused even though the row still says QUOTED —
        // same fail-closed derivation as reads
        let late = ts("2026-09-01T00:00:00Z");
        assert_eq!(accept(&quote, late), Err(AcceptError::Lapsed));

        // already accepted: sticky, refused forever after
        let mut accepted = quote.clone();
        accepted.status = QuoteStatus::Accepted;
        accepted.accepted_at = Some(now);
        assert_eq!(accept(&accepted, late), Err(AcceptError::AlreadyAccepted));
    }
}
