//! RFQ — the demand record (PLAN.md M1, DESIGN.md §2).
//!
//! A catalog miss is a structured, timestamped, identity-attributed statement
//! of willingness to pay for something that does not exist. Capture must be
//! frictionless — validation here is deliberately minimal (a real query and a
//! real buyer identity); everything else is optional signal. Never tax the
//! order book.

use serde::{Deserialize, Serialize};

/// A buyer-submitted RFQ, before the studio assigns identity and time.
/// Wire shape: `schemas/rfq.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRfq {
    /// The failed catalog search — the demand signal itself.
    pub query: String,
    /// What the buyer wants built.
    #[serde(default)]
    pub product: Option<String>,
    /// How the buyer thinks the artifact should be monetized.
    #[serde(default)]
    pub monetization: Option<String>,
    /// Competing/adjacent offerings the buyer knows about.
    #[serde(default)]
    pub competition: Vec<String>,
    /// Budget signal, not a commitment.
    #[serde(default)]
    pub budget_ceiling: Option<Amount>,
    /// Buyer identity — the npub it will later pay with.
    pub buyer_npub: String,
}

/// Token amount in minor units of `mint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Amount {
    pub amount: u64,
    /// SPL mint address (e.g. USDC). Free-form here; enforced at quote time.
    pub mint: String,
}

/// A captured demand record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rfq {
    pub id: String,
    pub query: String,
    pub product: Option<String>,
    pub monetization: Option<String>,
    pub competition: Vec<String>,
    pub budget_ceiling: Option<Amount>,
    pub buyer_npub: String,
    /// RFC 3339, UTC, server-assigned at capture.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// One field-level validation failure — serialized into 422 bodies.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FieldError {
    pub field: &'static str,
    pub message: String,
}

impl NewRfq {
    /// Frictionless-capture validation: only reject what would make the
    /// record useless (no query) or unattributable (no valid buyer key).
    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();

        if self.query.trim().is_empty() {
            errors.push(FieldError {
                field: "query",
                message: "must be a non-empty string".into(),
            });
        }

        if let Err(message) = validate_npub(&self.buyer_npub) {
            errors.push(FieldError {
                field: "buyer_npub",
                message,
            });
        }

        if let Some(budget) = &self.budget_ceiling {
            if budget.amount == 0 {
                errors.push(FieldError {
                    field: "budget_ceiling.amount",
                    message: "must be greater than zero when present".into(),
                });
            }
            if budget.mint.trim().is_empty() {
                errors.push(FieldError {
                    field: "budget_ceiling.mint",
                    message: "must be a non-empty mint address when present".into(),
                });
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
fn validate_npub(npub: &str) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ruben's real npub — a known-good bech32 vector.
    const GOOD_NPUB: &str = "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy";

    fn minimal() -> NewRfq {
        NewRfq {
            query: "solana priority fee forecast api".into(),
            product: None,
            monetization: None,
            competition: vec![],
            budget_ceiling: None,
            buyer_npub: GOOD_NPUB.into(),
        }
    }

    #[test]
    fn minimal_rfq_is_valid() {
        assert!(minimal().validate().is_ok());
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
            rfq.buyer_npub = bad.into();
            let errors = rfq.validate().unwrap_err();
            assert_eq!(errors[0].field, "buyer_npub", "input: {bad}");
        }
    }

    #[test]
    fn nsec_hrp_is_rejected() {
        // Right shape, wrong HRP — must not attribute demand to a secret key.
        let mut rfq = minimal();
        rfq.buyer_npub =
            bech32::encode::<bech32::Bech32>(bech32::Hrp::parse("nsec").unwrap(), &[7u8; 32])
                .unwrap();
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
            buyer_npub: "nope".into(),
        };
        let errors = rfq.validate().unwrap_err();
        let fields: Vec<_> = errors.iter().map(|e| e.field).collect();
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
