//! Capability brief — the intake interview's structured output (jude's
//! capability-request draft-00, thread 6873a1ec; slice 1).
//!
//! The brief rides an RFQ when the demand comes through the pay-side
//! intake path: the buyer's *own* model runs the interview, so the brief
//! is the buyer's signed representation, never something the studio must
//! trust. Three things dominate micro-agent opex — freshness (a cron burns
//! money whether or not anyone calls), paid upstream dependencies, and
//! data volume — so those are the load-bearing fields. Deliberately absent:
//! the buyer's willingness-to-pay (ruling A: the reserve price stays
//! pay-side and never reaches studios pre-quote).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::rfq::{Amount, FieldError};

/// The capability brief carried by an RFQ from the pay intake path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "Capability brief",
    description = "Structured output of the pay-side intake interview. example_exchange doubles as the delivery acceptance check (the default endpoint-live gate validates against it); freshness decides the monetization shape (scheduled work breaks scale-to-zero economics); upstream_dependencies carry the dominant opex."
)]
pub struct Brief {
    /// A mocked request/response of the endpoint the buyer wishes existed —
    /// simultaneously the studio's estimation input and the delivery
    /// acceptance test.
    pub example_exchange: ExampleExchange,
    pub freshness: Freshness,
    /// Paid third-party APIs the capability would sit on.
    #[serde(default)]
    pub upstream_dependencies: Vec<UpstreamDependency>,
    pub volume: VolumeBand,
    pub compute_class: ComputeClass,
    pub state: StateRequirement,
    pub interface: InterfaceKind,
}

/// The exchange the buyer wishes existed. Arbitrary JSON on both sides —
/// this is an example, not a schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExampleExchange {
    pub request: serde_json::Value,
    pub response: serde_json::Value,
}

/// How fresh the answer must be. Not a sizing detail: `scheduled` flips the
/// monetization shape from per-call to retainer, so the intake interview
/// branches on it.
// NOTE: no deny_unknown_fields — serde silently ignores it under internal
// tagging; strictness is enforced at the Brief container boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Freshness {
    Realtime,
    Cached {
        #[schemars(range(min = 1))]
        ttl_seconds: u64,
    },
    Scheduled {
        /// Cron expression for the refresh job.
        #[schemars(length(min = 1))]
        cron: String,
    },
}

/// One paid upstream the capability depends on. Cost per call is the one
/// number the buyer's agent should live-check rather than guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpstreamDependency {
    #[schemars(length(min = 1))]
    pub name: String,
    #[serde(default)]
    pub est_cost_per_call: Option<Amount>,
}

/// Estimated traffic, for sizing. Byte averages may be zero (a bodiless GET
/// has no request bytes); the call count may not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VolumeBand {
    #[schemars(range(min = 1))]
    pub calls_per_month: u64,
    pub avg_request_bytes: u64,
    pub avg_response_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComputeClass {
    Proxy,
    Cpu,
    Gpu,
}

/// What the capability must remember between calls.
// NOTE: no deny_unknown_fields — see `Freshness`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateRequirement {
    None,
    Cache,
    Durable {
        #[schemars(range(min = 1))]
        gib: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceKind {
    RequestResponse,
    WebhookPush,
    Dataset,
}

impl Brief {
    /// Structural validation, paths prefixed with `prefix` (the RFQ nests
    /// the brief, so errors read `brief.freshness.cron` etc.). The brief is
    /// agent-assembled, not human-typed — a present brief must be sound.
    pub fn validate(&self, prefix: &str) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();
        let mut push = |field: String, message: &str| {
            errors.push(FieldError {
                field,
                message: message.into(),
            });
        };

        match &self.freshness {
            Freshness::Realtime => {}
            Freshness::Cached { ttl_seconds } => {
                if *ttl_seconds == 0 {
                    push(
                        format!("{prefix}.freshness.ttl_seconds"),
                        "must be at least 1",
                    );
                }
            }
            Freshness::Scheduled { cron } => {
                if cron.trim().is_empty() {
                    push(
                        format!("{prefix}.freshness.cron"),
                        "must be a non-empty cron expression",
                    );
                }
            }
        }

        for (i, dep) in self.upstream_dependencies.iter().enumerate() {
            if dep.name.trim().is_empty() {
                push(
                    format!("{prefix}.upstream_dependencies[{i}].name"),
                    "must be non-empty",
                );
            }
            if let Some(cost) = &dep.est_cost_per_call {
                if cost.amount == 0 {
                    push(
                        format!("{prefix}.upstream_dependencies[{i}].est_cost_per_call.amount"),
                        "must be greater than zero when present",
                    );
                }
                if cost.mint.trim().is_empty() {
                    push(
                        format!("{prefix}.upstream_dependencies[{i}].est_cost_per_call.mint"),
                        "must be a non-empty mint address when present",
                    );
                }
            }
        }

        if self.volume.calls_per_month == 0 {
            push(
                format!("{prefix}.volume.calls_per_month"),
                "must be at least 1",
            );
        }

        if let StateRequirement::Durable { gib } = &self.state {
            if *gib == 0 {
                push(format!("{prefix}.state.gib"), "must be at least 1");
            }
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

    fn valid() -> Brief {
        Brief {
            example_exchange: ExampleExchange {
                request: serde_json::json!({ "program_id": "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4" }),
                response: serde_json::json!({ "p50_lamports": 12000, "p90_lamports": 55000 }),
            },
            freshness: Freshness::Cached { ttl_seconds: 30 },
            upstream_dependencies: vec![UpstreamDependency {
                name: "helius rpc".into(),
                est_cost_per_call: Some(Amount {
                    amount: 10,
                    mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
                }),
            }],
            volume: VolumeBand {
                calls_per_month: 50_000,
                avg_request_bytes: 0,
                avg_response_bytes: 512,
            },
            compute_class: ComputeClass::Cpu,
            state: StateRequirement::Cache,
            interface: InterfaceKind::RequestResponse,
        }
    }

    fn fields_of(brief: Brief) -> Vec<String> {
        brief
            .validate("brief")
            .unwrap_err()
            .into_iter()
            .map(|e| e.field)
            .collect()
    }

    #[test]
    fn valid_brief_passes_and_zero_request_bytes_are_fine() {
        assert!(valid().validate("brief").is_ok());
    }

    #[test]
    fn zero_cache_ttl_is_rejected() {
        let mut b = valid();
        b.freshness = Freshness::Cached { ttl_seconds: 0 };
        assert_eq!(fields_of(b), vec!["brief.freshness.ttl_seconds"]);
    }

    #[test]
    fn blank_cron_is_rejected() {
        let mut b = valid();
        b.freshness = Freshness::Scheduled { cron: "  ".into() };
        assert_eq!(fields_of(b), vec!["brief.freshness.cron"]);
    }

    #[test]
    fn upstream_dependency_errors_carry_indexed_paths() {
        let mut b = valid();
        b.upstream_dependencies.push(UpstreamDependency {
            name: " ".into(),
            est_cost_per_call: Some(Amount {
                amount: 0,
                mint: "".into(),
            }),
        });
        let fields = fields_of(b);
        for expected in [
            "brief.upstream_dependencies[1].name",
            "brief.upstream_dependencies[1].est_cost_per_call.amount",
            "brief.upstream_dependencies[1].est_cost_per_call.mint",
        ] {
            assert!(fields.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn zero_call_volume_and_zero_durable_gib_are_rejected() {
        let mut b = valid();
        b.volume.calls_per_month = 0;
        b.state = StateRequirement::Durable { gib: 0 };
        let fields = fields_of(b);
        assert!(fields.contains(&"brief.volume.calls_per_month".to_string()));
        assert!(fields.contains(&"brief.state.gib".to_string()));
    }

    #[test]
    fn wire_shape_round_trips_and_rejects_unknown_fields() {
        let json = serde_json::json!({
            "example_exchange": { "request": null, "response": { "ok": true } },
            "freshness": { "kind": "scheduled", "cron": "0 * * * *" },
            "volume": { "calls_per_month": 100, "avg_request_bytes": 0, "avg_response_bytes": 64 },
            "compute_class": "proxy",
            "state": { "kind": "durable", "gib": 2 },
            "interface": "dataset"
        });
        let brief: Brief = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(brief.upstream_dependencies, vec![]);
        assert_eq!(serde_json::from_value::<Brief>(json).unwrap(), brief);

        // Unknown fields are rejected at the brief level. (Inside the
        // kind-tagged enums serde cannot enforce deny_unknown_fields — a
        // documented serde limitation of internal tagging — so strictness
        // lives at the container boundary.)
        let bad = serde_json::json!({
            "example_exchange": { "request": null, "response": null },
            "freshness": { "kind": "realtime" },
            "volume": { "calls_per_month": 1, "avg_request_bytes": 0, "avg_response_bytes": 0 },
            "compute_class": "cpu",
            "state": { "kind": "none" },
            "interface": "request_response",
            "surprise": 1
        });
        assert!(serde_json::from_value::<Brief>(bad).is_err());
    }
}
