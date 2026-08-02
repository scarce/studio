//! Quote — the studio's priced answer to an RFQ (PLAN.md M2, DESIGN.md §3).
//!
//! Milestone granularity is the entire dispute system, so the milestone
//! schedule must account for every unit of the price. The gate policy rides
//! in the quote because it is negotiated there: defaults from studio config,
//! buyers may strengthen. `payout_destination` is enum-shaped so the v1
//! vault slots in without a breaking change (ARCHITECTURE.md §2.3).

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::gate::GatePolicy;
use crate::rfq::{Amount, FieldError};

/// A studio-authored quote, before the studio assigns identity and time.
/// Wire shape: `schemas/quote.json` — generated from this type.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "Quote submission",
    description = "The studio's priced answer to an RFQ (PLAN.md M2, DESIGN.md §3). Wire shape of POST /api/v1/rfqs/{id}/quote; the studio assigns id, created_at, status, and policy_hash. The implementation is additionally stricter than this schema: milestone amounts must sum to price.amount (milestone granularity is the dispute system), split bps must sum to exactly 10000, expires_at must be in the future at issue time, and the gate policy's structural rules (see gate-policy.json) are enforced."
)]
pub struct NewQuote {
    /// Total engagement price. Must equal the sum of milestone amounts.
    pub price: Amount,
    #[schemars(length(min = 1))]
    pub milestones: Vec<MilestoneSpec>,
    /// Human-negotiated schedule description (e.g. "3 weeks, weekly demos").
    #[schemars(length(min = 1))]
    pub timeline: String,
    pub payout_destination: PayoutDestination,
    pub channel: ChannelParams,
    /// The 402-gated URL acceptance opens the MPP session against (jude's
    /// capability-request draft-00 §4): deposit = price, terms from `channel`,
    /// payee = `payout_destination`.
    #[schemars(length(min = 1), extend("format" = "uri"))]
    pub engagement_endpoint: String,
    /// Defaults from studio config; buyers may strengthen per-project.
    #[serde(default = "GatePolicy::studio_default")]
    pub gate_policy: GatePolicy,
    /// Past this instant the quote is LAPSED and cannot be accepted.
    pub expires_at: DateTime<Utc>,
}

/// One milestone: a demoable, acceptable, priced unit of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MilestoneSpec {
    #[schemars(length(min = 1))]
    pub title: String,
    #[schemars(length(min = 1))]
    pub description: String,
    /// Minor units of the quote's `price.mint`.
    #[schemars(range(min = 1))]
    pub amount: u64,
}

/// Where settled funds go. v0: direct channel splits, exactly DESIGN.md
/// §4.3(a) — escrow pays the crew, no studio custody. The `vault` variant
/// (v1, ARCHITECTURE.md §2.3) will slot in beside `splits`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PayoutDestination {
    Splits {
        #[schemars(length(min = 1))]
        splits: Vec<Split>,
    },
}

/// One recipient's share, in basis points. Splits must sum to exactly
/// 10_000 bps — every lamport of a settlement is accounted for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Split {
    /// Solana address. Free-form here, like `Amount::mint`; enforced when
    /// the live PayPort builds real session terms (M5).
    #[schemars(length(min = 1))]
    pub recipient: String,
    #[schemars(range(min = 1, max = 10_000))]
    pub bps: u32,
}

/// MPP session channel parameters the quote commits to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelParams {
    /// Buyer-exit grace window, seconds. Default 172800 (48h) per PLAN.md M2.
    #[serde(default = "default_grace_seconds")]
    #[schemars(range(min = 1))]
    pub grace_seconds: u64,
    /// Idle window after which the studio settles at watermark and closes.
    /// No default on purpose: the quote must commit to it explicitly.
    #[schemars(range(min = 1))]
    pub idle_timeout_seconds: u64,
}

fn default_grace_seconds() -> u64 {
    172_800
}

/// Quote lifecycle (PLAN.md §2): issued → QUOTED; expiry sweep or read-side
/// derivation → LAPSED; buyer acceptance → ACCEPTED (sticky — an accepted
/// quote never lapses; the engagement it started owns the clock from there).
/// ACCEPTED stands in for FUNDED while payments are stubbed (PLAN.md §6
/// override path, ludovic 2026-08-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum QuoteStatus {
    Quoted,
    Lapsed,
    Accepted,
}

/// An issued quote — what the quote endpoints return.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "Quote record")]
pub struct Quote {
    pub id: String,
    pub rfq_id: String,
    pub price: Amount,
    pub milestones: Vec<MilestoneSpec>,
    pub timeline: String,
    pub payout_destination: PayoutDestination,
    pub channel: ChannelParams,
    pub engagement_endpoint: String,
    pub gate_policy: GatePolicy,
    /// `studio-core::gate::commitment_hash(&gate_policy)`, precomputed at
    /// issue time. Recorded again (and enforced) at the FUNDED transition —
    /// PLAN.md §2.1(4).
    pub policy_hash: String,
    /// SHA-256 (hex) over the canonical JSON of the quote's immutable
    /// commitment fields, in this exact order: id, rfq_id, price,
    /// milestones, timeline, payout_destination, channel,
    /// engagement_endpoint, policy_hash, expires_at (lifecycle fields —
    /// status, created_at, lapsed_at, accepted_at — are excluded). Same
    /// canonicalization as policy_hash. Session terms hash-commit the quote
    /// through this value at accept, so what is funded is provably what was
    /// quoted. Derived from the fields, never stored — it cannot go stale.
    pub quote_hash: String,
    pub expires_at: DateTime<Utc>,
    pub status: QuoteStatus,
    pub created_at: DateTime<Utc>,
    /// Set by the expiry sweep; `expires_at` when derived at read time.
    pub lapsed_at: Option<DateTime<Utc>>,
    /// Buyer acceptance instant. Set exactly once; never on a lapsed quote.
    pub accepted_at: Option<DateTime<Utc>>,
}

/// The immutable commitment fields of a quote, in the exact serialization
/// order `quote_hash` documents. A serialize-only view: adding a lifecycle
/// field to `Quote` cannot silently change the hash, and an external
/// verifier can rebuild this object from the published schema description
/// alone.
#[derive(Serialize)]
struct QuoteCommitment<'a> {
    id: &'a str,
    rfq_id: &'a str,
    price: &'a Amount,
    milestones: &'a [MilestoneSpec],
    timeline: &'a str,
    payout_destination: &'a PayoutDestination,
    channel: &'a ChannelParams,
    engagement_endpoint: &'a str,
    policy_hash: &'a str,
    expires_at: &'a DateTime<Utc>,
}

impl Quote {
    /// The status as of `now`, fail-closed against sweep lag: a quote past
    /// `expires_at` reads LAPSED even if the sweep has not stamped it yet.
    /// ACCEPTED is sticky — acceptance beat expiry, so expiry is moot.
    pub fn at(mut self, now: DateTime<Utc>) -> Quote {
        if self.status == QuoteStatus::Quoted && now >= self.expires_at {
            self.status = QuoteStatus::Lapsed;
            self.lapsed_at = Some(self.expires_at);
        }
        self
    }

    /// Compute the commitment hash from the immutable fields (see the
    /// `quote_hash` field docs for the exact input). Lives here rather than
    /// `studio-core` because the store derives it on every row read and
    /// depends only on this crate — and pay-side verifiers get it from the
    /// contract crate for free.
    pub fn commitment_hash(&self) -> String {
        let commitment = QuoteCommitment {
            id: &self.id,
            rfq_id: &self.rfq_id,
            price: &self.price,
            milestones: &self.milestones,
            timeline: &self.timeline,
            payout_destination: &self.payout_destination,
            channel: &self.channel,
            engagement_endpoint: &self.engagement_endpoint,
            policy_hash: &self.policy_hash,
            expires_at: &self.expires_at,
        };
        let canonical = serde_json::to_vec(&commitment).expect("QuoteCommitment serializes");
        let digest = Sha256::digest(&canonical);
        digest.iter().fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write;
            write!(s, "{b:02x}").expect("writing to a String cannot fail");
            s
        })
    }

    /// Fill `quote_hash` from the other fields — the single sealing step
    /// every constructor path (issuance, row read) goes through.
    pub fn with_commitment_hash(mut self) -> Quote {
        self.quote_hash = self.commitment_hash();
        self
    }
}

impl NewQuote {
    /// Full structural validation. `now` anchors the expiry check — pure,
    /// like everything in this crate.
    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();
        let mut push = |field: &str, message: &str| {
            errors.push(FieldError {
                field: field.into(),
                message: message.into(),
            });
        };

        if self.price.amount == 0 {
            push("price.amount", "must be greater than zero");
        }
        if self.price.mint.trim().is_empty() {
            push("price.mint", "must be a non-empty mint address");
        }

        if self.milestones.is_empty() {
            push("milestones", "must contain at least one milestone");
        }
        for (i, m) in self.milestones.iter().enumerate() {
            if m.title.trim().is_empty() {
                push(&format!("milestones[{i}].title"), "must be non-empty");
            }
            if m.description.trim().is_empty() {
                push(&format!("milestones[{i}].description"), "must be non-empty");
            }
            if m.amount == 0 {
                push(
                    &format!("milestones[{i}].amount"),
                    "must be greater than zero",
                );
            }
        }
        let milestone_sum: u128 = self.milestones.iter().map(|m| u128::from(m.amount)).sum();
        if !self.milestones.is_empty() && milestone_sum != u128::from(self.price.amount) {
            push(
                "milestones",
                &format!(
                    "amounts must sum to price.amount — the milestone schedule is the \
                     dispute system, every unit must be accounted for \
                     (sum {milestone_sum}, price {})",
                    self.price.amount
                ),
            );
        }

        if self.timeline.trim().is_empty() {
            push("timeline", "must be non-empty");
        }

        match url::Url::parse(&self.engagement_endpoint) {
            Err(_) => push(
                "engagement_endpoint",
                "must be an absolute URL (the 402-gated endpoint acceptance \
                 opens the session against)",
            ),
            Ok(parsed) => {
                if !matches!(parsed.scheme(), "http" | "https") {
                    push(
                        "engagement_endpoint",
                        "must use the http or https scheme (sessions open over HTTP 402)",
                    );
                } else if parsed.host_str().is_none() {
                    push("engagement_endpoint", "must include a host");
                }
            }
        }

        match &self.payout_destination {
            PayoutDestination::Splits { splits } => {
                if splits.is_empty() {
                    push(
                        "payout_destination.splits",
                        "must contain at least one recipient",
                    );
                }
                for (i, s) in splits.iter().enumerate() {
                    if s.recipient.trim().is_empty() {
                        push(
                            &format!("payout_destination.splits[{i}].recipient"),
                            "must be a non-empty address",
                        );
                    }
                    if s.bps == 0 {
                        push(
                            &format!("payout_destination.splits[{i}].bps"),
                            "must be at least 1",
                        );
                    }
                }
                let bps_sum: u64 = splits.iter().map(|s| u64::from(s.bps)).sum();
                if !splits.is_empty() && bps_sum != 10_000 {
                    push(
                        "payout_destination.splits",
                        &format!("bps must sum to exactly 10000, got {bps_sum}"),
                    );
                }
            }
        }

        if self.channel.grace_seconds == 0 {
            push("channel.grace_seconds", "must be at least 1");
        }
        if self.channel.idle_timeout_seconds == 0 {
            push("channel.idle_timeout_seconds", "must be at least 1");
        }

        if let Err(policy_errors) = self.gate_policy.validate("gate_policy") {
            errors.extend(policy_errors);
        }

        if self.expires_at <= now {
            errors.push(FieldError {
                field: "expires_at".into(),
                message: "must be in the future".into(),
            });
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    const NOW: &str = "2026-08-01T15:00:00Z";
    const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    fn valid() -> NewQuote {
        NewQuote {
            price: Amount {
                amount: 250_000_000,
                mint: USDC.into(),
            },
            milestones: vec![
                MilestoneSpec {
                    title: "Forecast model".into(),
                    description: "p50/p90 fee forecast per program id, backtested".into(),
                    amount: 150_000_000,
                },
                MilestoneSpec {
                    title: "Gated endpoint".into(),
                    description: "pay.sh-gated REST endpoint answering its 402 challenge".into(),
                    amount: 100_000_000,
                },
            ],
            timeline: "2 weeks, weekly demos".into(),
            payout_destination: PayoutDestination::Splits {
                splits: vec![
                    Split {
                        recipient: "CrewAgentA111111111111111111111111111111111".into(),
                        bps: 7_000,
                    },
                    Split {
                        recipient: "CrewAgentB111111111111111111111111111111111".into(),
                        bps: 3_000,
                    },
                ],
            },
            channel: ChannelParams {
                grace_seconds: 172_800,
                idle_timeout_seconds: 604_800,
            },
            engagement_endpoint: "https://scarce.sh/api/v1/engagements/rfq-1".into(),
            gate_policy: GatePolicy::studio_default(),
            expires_at: ts("2026-08-08T15:00:00Z"),
        }
    }

    fn errors_of(quote: NewQuote) -> Vec<String> {
        quote
            .validate(ts(NOW))
            .unwrap_err()
            .into_iter()
            .map(|e| e.field)
            .collect()
    }

    #[test]
    fn valid_quote_passes() {
        assert!(valid().validate(ts(NOW)).is_ok());
    }

    #[test]
    fn milestones_must_sum_to_the_price() {
        let mut q = valid();
        q.milestones[1].amount = 99_000_000;
        assert!(errors_of(q).contains(&"milestones".to_string()));
    }

    #[test]
    fn milestone_sum_does_not_overflow() {
        let mut q = valid();
        q.milestones[0].amount = u64::MAX;
        q.milestones[1].amount = u64::MAX;
        assert!(errors_of(q).contains(&"milestones".to_string()));
    }

    #[test]
    fn empty_milestones_are_rejected() {
        let mut q = valid();
        q.milestones.clear();
        assert!(errors_of(q).contains(&"milestones".to_string()));
    }

    #[test]
    fn milestone_fields_are_validated_with_indexed_paths() {
        let mut q = valid();
        q.milestones[1] = MilestoneSpec {
            title: " ".into(),
            description: "".into(),
            amount: 0,
        };
        let fields = errors_of(q);
        for expected in [
            "milestones[1].title",
            "milestones[1].description",
            "milestones[1].amount",
        ] {
            assert!(fields.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn splits_must_sum_to_10000_bps() {
        let mut q = valid();
        q.payout_destination = PayoutDestination::Splits {
            splits: vec![Split {
                recipient: "CrewAgentA111111111111111111111111111111111".into(),
                bps: 9_999,
            }],
        };
        assert!(errors_of(q).contains(&"payout_destination.splits".to_string()));
    }

    #[test]
    fn zero_bps_and_empty_recipient_are_rejected() {
        let mut q = valid();
        q.payout_destination = PayoutDestination::Splits {
            splits: vec![
                Split {
                    recipient: "".into(),
                    bps: 0,
                },
                Split {
                    recipient: "CrewAgentA111111111111111111111111111111111".into(),
                    bps: 10_000,
                },
            ],
        };
        let fields = errors_of(q);
        assert!(fields.contains(&"payout_destination.splits[0].recipient".to_string()));
        assert!(fields.contains(&"payout_destination.splits[0].bps".to_string()));
    }

    #[test]
    fn empty_splits_are_rejected() {
        let mut q = valid();
        q.payout_destination = PayoutDestination::Splits { splits: vec![] };
        assert!(errors_of(q).contains(&"payout_destination.splits".to_string()));
    }

    #[test]
    fn expired_expiry_zero_price_blank_timeline_and_zero_windows_are_rejected() {
        let mut q = valid();
        q.expires_at = ts("2026-08-01T14:59:59Z");
        q.price = Amount {
            amount: 0,
            mint: " ".into(),
        };
        q.timeline = "  ".into();
        q.channel.grace_seconds = 0;
        q.channel.idle_timeout_seconds = 0;
        q.milestones = vec![MilestoneSpec {
            title: "m".into(),
            description: "d".into(),
            amount: 1,
        }];
        let fields = errors_of(q);
        for expected in [
            "price.amount",
            "price.mint",
            "timeline",
            "channel.grace_seconds",
            "channel.idle_timeout_seconds",
            "expires_at",
        ] {
            assert!(fields.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn invalid_gate_policy_fails_the_quote_with_prefixed_paths() {
        let mut q = valid();
        q.gate_policy.edges.insert("QUOTED->QUOTED".into(), vec![]);
        let fields = errors_of(q);
        assert!(fields
            .iter()
            .any(|f| f.starts_with("gate_policy.edges[QUOTED->QUOTED]")));
    }

    #[test]
    fn omitted_gate_policy_and_grace_default_correctly() {
        let json = serde_json::json!({
            "price": { "amount": 100, "mint": USDC },
            "milestones": [
                { "title": "all of it", "description": "one milestone", "amount": 100 }
            ],
            "timeline": "3 days",
            "payout_destination": { "kind": "splits", "splits": [
                { "recipient": "CrewAgentA111111111111111111111111111111111", "bps": 10000 }
            ]},
            "channel": { "idle_timeout_seconds": 3600 },
            "engagement_endpoint": "https://scarce.sh/api/v1/engagements/rfq-1",
            "expires_at": "2026-08-08T15:00:00Z"
        });
        let q: NewQuote = serde_json::from_value(json).unwrap();
        assert_eq!(q.channel.grace_seconds, 172_800);
        assert_eq!(q.gate_policy, GatePolicy::studio_default());
        assert!(q.validate(ts(NOW)).is_ok());
    }

    #[test]
    fn unknown_fields_are_rejected_everywhere() {
        for bad in [
            serde_json::json!({ "price": { "amount": 1, "mint": USDC }, "surprise": 1 }),
            serde_json::json!({ "payout_destination": { "kind": "splits", "splits": [], "vault": "x" } }),
            serde_json::json!({ "channel": { "idle_timeout_seconds": 1, "surprise": 1 } }),
        ] {
            assert!(
                serde_json::from_value::<NewQuote>(bad.clone()).is_err(),
                "should reject {bad}"
            );
        }
    }

    fn record() -> Quote {
        Quote {
            id: "q-1".into(),
            rfq_id: "r-1".into(),
            price: Amount {
                amount: 1,
                mint: USDC.into(),
            },
            milestones: vec![],
            timeline: "t".into(),
            payout_destination: PayoutDestination::Splits { splits: vec![] },
            channel: ChannelParams {
                grace_seconds: 1,
                idle_timeout_seconds: 1,
            },
            engagement_endpoint: "https://scarce.sh/api/v1/engagements/r-1".into(),
            gate_policy: GatePolicy::studio_default(),
            policy_hash: "policy-hash-set-at-issue-time".into(),
            quote_hash: String::new(),
            expires_at: ts("2026-08-02T00:00:00Z"),
            status: QuoteStatus::Quoted,
            created_at: ts("2026-08-01T00:00:00Z"),
            lapsed_at: None,
            accepted_at: None,
        }
        .with_commitment_hash()
    }

    #[test]
    fn missing_or_malformed_engagement_endpoint_is_rejected() {
        for bad in ["", "not a url", "ftp://scarce.sh/x", "scarce.sh/relative"] {
            let mut q = valid();
            q.engagement_endpoint = bad.into();
            assert!(
                errors_of(q).contains(&"engagement_endpoint".to_string()),
                "should reject {bad:?}"
            );
        }
        // and the field is required on the wire, not defaulted
        let mut json = serde_json::to_value(valid()).unwrap();
        json.as_object_mut().unwrap().remove("engagement_endpoint");
        assert!(serde_json::from_value::<NewQuote>(json).is_err());
    }

    #[test]
    fn commitment_hash_is_deterministic_and_covers_the_commitment_fields() {
        let q = record();
        assert_eq!(q.quote_hash, q.commitment_hash());
        assert_eq!(q.quote_hash.len(), 64);
        assert_eq!(q.quote_hash, record().quote_hash);

        // lifecycle fields do not move the hash…
        let mut accepted = record();
        accepted.status = QuoteStatus::Accepted;
        accepted.accepted_at = Some(ts("2026-08-01T12:00:00Z"));
        assert_eq!(accepted.commitment_hash(), q.quote_hash);

        // …commitment fields do
        for mutated in [
            {
                let mut m = record();
                m.price.amount = 2;
                m
            },
            {
                let mut m = record();
                m.engagement_endpoint = "https://scarce.sh/api/v1/engagements/other".into();
                m
            },
            {
                let mut m = record();
                m.policy_hash = "different".into();
                m
            },
        ] {
            assert_ne!(mutated.commitment_hash(), q.quote_hash);
        }
    }

    #[test]
    fn effective_status_derives_lapsed_past_expiry() {
        let q = record();

        let live = q.clone().at(ts("2026-08-01T23:59:59Z"));
        assert_eq!(live.status, QuoteStatus::Quoted);
        assert_eq!(live.lapsed_at, None);

        let lapsed = q.clone().at(ts("2026-08-02T00:00:00Z"));
        assert_eq!(lapsed.status, QuoteStatus::Lapsed);
        assert_eq!(lapsed.lapsed_at, Some(ts("2026-08-02T00:00:00Z")));

        // an already-swept quote keeps its stamped lapsed_at
        let mut swept = q.clone();
        swept.status = QuoteStatus::Lapsed;
        swept.lapsed_at = Some(ts("2026-08-02T00:00:30Z"));
        let read = swept.at(ts("2026-08-03T00:00:00Z"));
        assert_eq!(read.lapsed_at, Some(ts("2026-08-02T00:00:30Z")));
    }
}
