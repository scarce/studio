//! RFQ — the demand record (PLAN.md M1, DESIGN.md §2).
//!
//! A catalog miss is a structured, timestamped, identity-attributed statement
//! of willingness to pay for something that does not exist. Capture must be
//! frictionless — validation here is deliberately minimal (a real query and a
//! real buyer identity); everything else is optional signal. Never tax the
//! order book.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::brief::Brief;

/// Structural npub pattern for the JSON Schema. The bech32 charset excludes
/// `1`, `b`, `i`, `o`; `validate()` additionally checks the checksum, which a
/// regex cannot.
pub const NPUB_PATTERN: &str = "^npub1[02-9ac-hj-np-z]{58}$";

/// Structural Solana pubkey pattern for the JSON Schema (base58, 32 bytes
/// encodes to 32–44 chars); `validate()` additionally decodes, which a regex
/// cannot.
pub const SOLANA_PUBKEY_PATTERN: &str = "^[1-9A-HJ-NP-Za-km-z]{32,44}$";

/// A buyer-submitted RFQ, before the studio assigns identity and time.
/// Wire shape: `schemas/rfq.json` — generated from this type.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "RFQ submission",
    description = "A demand record captured from a pay.sh catalog miss. Capture is deliberately frictionless: only the query and the buyer identity are required (DESIGN.md §2 — never tax the order book). This is the wire shape of POST /api/v1/rfqs; the studio assigns id and created_at."
)]
pub struct NewRfq {
    /// The failed catalog search — the demand signal itself.
    #[schemars(length(min = 1))]
    pub query: String,
    /// What the buyer wants built.
    #[serde(default)]
    pub product: Option<String>,
    /// How the buyer thinks the artifact should be monetized.
    #[serde(default)]
    pub monetization: Option<String>,
    /// Competing or adjacent offerings the buyer knows about.
    #[serde(default)]
    pub competition: Vec<String>,
    /// Budget signal, not a commitment.
    #[serde(default)]
    pub budget_ceiling: Option<Amount>,
    /// Buyer identity, Nostr side. At least one of `buyer_npub` /
    /// `buyer_solana_pubkey` is required — the commission path (jude's
    /// draft-00) submits with the Solana key it will pay with, direct
    /// captures keep using the npub.
    #[serde(default)]
    #[schemars(regex(pattern = NPUB_PATTERN))]
    pub buyer_npub: Option<String>,
    /// Buyer identity, Solana side — the ed25519 key the buyer will fund
    /// the engagement with. Structural check here (base58, 32 bytes);
    /// on-curve is implied by signature verification when it lands
    /// (slice 2).
    #[serde(default)]
    #[schemars(regex(pattern = SOLANA_PUBKEY_PATTERN))]
    pub buyer_solana_pubkey: Option<String>,
    /// Reserved for the buyer-authored upgrade path (archy, 2026-08-01 M1
    /// boundary): a SIWX-style signature over the submission by
    /// `buyer_npub`, making the RFQ counterparty-signed substrate instead
    /// of studio self-attestation (ARCHITECTURE.md §1). Recorded, not yet
    /// verified — like the delivery attestation field (PLAN.md §0).
    #[serde(default)]
    #[schemars(length(min = 1))]
    pub buyer_signature: Option<String>,
    /// Commission brief from the pay-side intake interview. Optional —
    /// direct captures stay frictionless; when present it must be
    /// structurally sound (it is agent-assembled, not human-typed).
    #[serde(default)]
    pub brief: Option<Brief>,
}

/// Token amount in minor units of `mint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Amount {
    /// Token amount in minor units of `mint`.
    #[schemars(range(min = 1))]
    pub amount: u64,
    /// SPL mint address (e.g. USDC). Free-form here; enforced at quote time.
    #[schemars(length(min = 1))]
    pub mint: String,
}

/// A captured demand record — what `POST /api/v1/rfqs` returns.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "RFQ record")]
pub struct Rfq {
    pub id: String,
    pub query: String,
    pub product: Option<String>,
    pub monetization: Option<String>,
    pub competition: Vec<String>,
    pub budget_ceiling: Option<Amount>,
    /// At least one buyer identity is always present (capture refuses
    /// otherwise); which one depends on the intake path.
    pub buyer_npub: Option<String>,
    pub buyer_solana_pubkey: Option<String>,
    /// Reserved (see `NewRfq::buyer_signature`); recorded, not verified.
    pub buyer_signature: Option<String>,
    pub brief: Option<Brief>,
    /// RFC 3339, UTC, server-assigned at capture.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// One field-level validation failure — serialized into 422 bodies.
/// `field` is a path (e.g. `milestones[1].amount`), so it is owned.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, JsonSchema)]
pub struct FieldError {
    /// Path of the offending field (e.g. `milestones[1].amount`).
    pub field: String,
    /// What a valid value looks like — actionable, not a bare "invalid".
    pub message: String,
}

impl NewRfq {
    /// Frictionless-capture validation: only reject what would make the
    /// record useless (no query) or unattributable (no valid buyer key).
    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();

        if self.query.trim().is_empty() {
            errors.push(FieldError {
                field: "query".into(),
                message: "must be a non-empty string".into(),
            });
        }

        if self.buyer_npub.is_none() && self.buyer_solana_pubkey.is_none() {
            errors.push(FieldError {
                field: "buyer_npub".into(),
                message: "at least one buyer identity is required \
                          (buyer_npub or buyer_solana_pubkey)"
                    .into(),
            });
        }
        if let Some(npub) = &self.buyer_npub {
            if let Err(message) = validate_npub(npub) {
                errors.push(FieldError {
                    field: "buyer_npub".into(),
                    message,
                });
            }
        }
        if let Some(pubkey) = &self.buyer_solana_pubkey {
            if let Err(message) = validate_solana_pubkey(pubkey) {
                errors.push(FieldError {
                    field: "buyer_solana_pubkey".into(),
                    message,
                });
            }
        }

        if let Some(budget) = &self.budget_ceiling {
            if budget.amount == 0 {
                errors.push(FieldError {
                    field: "budget_ceiling.amount".into(),
                    message: "must be greater than zero when present".into(),
                });
            }
            if budget.mint.trim().is_empty() {
                errors.push(FieldError {
                    field: "budget_ceiling.mint".into(),
                    message: "must be a non-empty mint address when present".into(),
                });
            }
        }

        if let Some(sig) = &self.buyer_signature {
            if sig.trim().is_empty() {
                errors.push(FieldError {
                    field: "buyer_signature".into(),
                    message: "must be non-empty when present (omit it instead)".into(),
                });
            }
        }

        if let Some(brief) = &self.brief {
            if let Err(brief_errors) = brief.validate("brief") {
                errors.extend(brief_errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Structural npub check: bech32 decodes, HRP is `npub`, payload is 32 bytes.
/// Signature verification is a substrate concern and arrives with the
/// orchestrator (M3); identity attribution only needs a well-formed key.
/// Public because the agent registry (roster.toml) binds persona names to
/// npubs with the same structural rule.
pub fn validate_npub(npub: &str) -> Result<(), String> {
    let (hrp, data) =
        bech32::decode(npub).map_err(|_| "must be a bech32 npub (npub1…)".to_string())?;
    if hrp.as_str() != "npub" {
        return Err(format!("expected npub, got {hrp}"));
    }
    if data.len() != 32 {
        return Err("npub payload must be 32 bytes".to_string());
    }
    Ok(())
}

/// Structural Solana pubkey check: base58 decodes to exactly 32 bytes.
/// Deliberately no on-curve check — that is what signature verification
/// proves (slice 2); a key that never signs never funds anything.
pub fn validate_solana_pubkey(pubkey: &str) -> Result<(), String> {
    let bytes = bs58::decode(pubkey)
        .into_vec()
        .map_err(|_| "must be a base58 Solana pubkey".to_string())?;
    if bytes.len() != 32 {
        return Err(format!(
            "must decode to 32 bytes, got {} — not an ed25519 pubkey",
            bytes.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ruben's real npub — a known-good bech32 vector.
    const GOOD_NPUB: &str = "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy";

    // A well-known 32-byte base58 vector (the USDC mint) — shape-valid.
    const GOOD_SOLANA: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    fn minimal() -> NewRfq {
        NewRfq {
            query: "solana priority fee forecast api".into(),
            product: None,
            monetization: None,
            competition: vec![],
            budget_ceiling: None,
            buyer_npub: Some(GOOD_NPUB.into()),
            buyer_solana_pubkey: None,
            buyer_signature: None,
            brief: None,
        }
    }

    #[test]
    fn minimal_rfq_is_valid() {
        assert!(minimal().validate().is_ok());
    }

    #[test]
    fn solana_only_identity_is_valid() {
        let mut rfq = minimal();
        rfq.buyer_npub = None;
        rfq.buyer_solana_pubkey = Some(GOOD_SOLANA.into());
        assert!(rfq.validate().is_ok());
    }

    #[test]
    fn missing_both_identities_is_rejected() {
        let mut rfq = minimal();
        rfq.buyer_npub = None;
        let errors = rfq.validate().unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "buyer_npub");
        assert!(errors[0].message.contains("buyer_solana_pubkey"));
    }

    #[test]
    fn bad_solana_pubkey_is_rejected_even_alongside_a_good_npub() {
        // 0, O, I, l are outside the base58 alphabet; "abc" decodes short.
        for bad in ["", "abc", "0OIl0OIl0OIl0OIl0OIl0OIl0OIl0OIl"] {
            let mut rfq = minimal();
            rfq.buyer_solana_pubkey = Some(bad.into());
            let errors = rfq.validate().unwrap_err();
            assert_eq!(errors[0].field, "buyer_solana_pubkey", "input: {bad}");
        }
    }

    #[test]
    fn invalid_brief_fails_the_rfq_with_prefixed_paths() {
        let mut rfq = minimal();
        rfq.brief = Some(crate::brief::Brief {
            example_exchange: crate::brief::ExampleExchange {
                request: serde_json::Value::Null,
                response: serde_json::Value::Null,
            },
            freshness: crate::brief::Freshness::Cached { ttl_seconds: 0 },
            upstream_dependencies: vec![],
            volume: crate::brief::VolumeBand {
                calls_per_month: 1,
                avg_request_bytes: 0,
                avg_response_bytes: 0,
            },
            compute_class: crate::brief::ComputeClass::Proxy,
            state: crate::brief::StateRequirement::None,
            interface: crate::brief::InterfaceKind::RequestResponse,
        });
        let errors = rfq.validate().unwrap_err();
        assert_eq!(errors[0].field, "brief.freshness.ttl_seconds");
    }

    #[test]
    fn empty_query_is_rejected_with_field_error() {
        let mut rfq = minimal();
        rfq.query = "   ".into();
        let errors = rfq.validate().unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "query");
    }

    #[test]
    fn bad_npub_is_rejected() {
        let truncated = &GOOD_NPUB[..GOOD_NPUB.len() - 2];
        for bad in ["", "npub1notbech32!!!", "hello", truncated] {
            let mut rfq = minimal();
            rfq.buyer_npub = Some(bad.into());
            let errors = rfq.validate().unwrap_err();
            assert_eq!(errors[0].field, "buyer_npub", "input: {bad}");
        }
    }

    #[test]
    fn nsec_hrp_is_rejected() {
        // Right shape, wrong HRP — must not attribute demand to a secret key.
        let mut rfq = minimal();
        rfq.buyer_npub = Some(
            bech32::encode::<bech32::Bech32>(bech32::Hrp::parse("nsec").unwrap(), &[7u8; 32])
                .unwrap(),
        );
        let errors = rfq.validate().unwrap_err();
        assert_eq!(errors[0].field, "buyer_npub");
    }

    #[test]
    fn zero_budget_is_rejected_but_absent_budget_is_fine() {
        let mut rfq = minimal();
        rfq.budget_ceiling = Some(Amount {
            amount: 0,
            mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
        });
        let errors = rfq.validate().unwrap_err();
        assert_eq!(errors[0].field, "budget_ceiling.amount");
    }

    #[test]
    fn all_errors_are_collected_not_first_only() {
        let rfq = NewRfq {
            query: "".into(),
            product: None,
            monetization: None,
            competition: vec![],
            budget_ceiling: Some(Amount {
                amount: 0,
                mint: "".into(),
            }),
            buyer_npub: Some("nope".into()),
            buyer_solana_pubkey: None,
            buyer_signature: None,
            brief: None,
        };
        let errors = rfq.validate().unwrap_err();
        let fields: Vec<_> = errors.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(
            fields,
            vec![
                "query",
                "buyer_npub",
                "budget_ceiling.amount",
                "budget_ceiling.mint"
            ]
        );
    }
}
