//! Gate policy — the wire types of a project's procedural law (PLAN.md
//! §2.1). Wire shape: `schemas/gate-policy.json` — generated from these
//! types. The engine that *evaluates* a policy against evidence lives in
//! `studio-core::gate`; this module owns the shape, its field validation,
//! and the studio default.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::rfq::FieldError;
use crate::state::{EdgePattern, ProjectState};

/// One gate on a state-machine edge (internally tagged on `type`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum GateSpec {
    /// A named principal must approve. Primary mechanism: native Buzz
    /// workflow approval tokens; fallback: signed channel message with fixed
    /// grammar. The principal is an opaque label (npub or a role like
    /// `buyer`) — evidence must carry the same label.
    HumanApproval {
        /// Opaque principal label — an npub or a role like `buyer`.
        /// Evidence must carry the same label.
        #[schemars(length(min = 1))]
        principal: String,
        /// Blocked longer than this (from edge eligibility) → escalate to
        /// the gate's principal, then the studio owner. Never auto-passes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 1))]
        escalate_after_seconds: Option<u64>,
    },
    /// k-of-n signed sign-offs from named crew agents.
    AgentSignoff {
        #[schemars(length(min = 1))]
        agents: Vec<String>,
        #[schemars(range(min = 1))]
        k: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 1))]
        escalate_after_seconds: Option<u64>,
    },
    /// A named verifiable predicate (CI green, endpoint answers its 402
    /// challenge, schema validation passes). Reruns are natural, so the
    /// latest result wins; a stale pass is expired evidence and blocks.
    MachineCheck {
        #[schemars(length(min = 1))]
        check: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 1))]
        max_age_seconds: Option<u64>,
    },
    /// `PayPort` status: operator record in stub mode, tx signature live.
    PaymentEvidence,
    /// Minimum elapsed review window since the edge became eligible.
    Timelock {
        #[schemars(range(min = 1))]
        min_seconds: u64,
    },
}

impl GateSpec {
    /// The escalation window, for gate kinds that have one.
    pub fn escalate_after_seconds(&self) -> Option<u64> {
        match self {
            GateSpec::HumanApproval {
                escalate_after_seconds,
                ..
            }
            | GateSpec::AgentSignoff {
                escalate_after_seconds,
                ..
            } => *escalate_after_seconds,
            _ => None,
        }
    }

    fn validate(&self, field: &str, errors: &mut Vec<FieldError>) {
        let mut push = |suffix: &str, message: &str| {
            errors.push(FieldError {
                field: format!("{field}.{suffix}"),
                message: message.into(),
            });
        };
        match self {
            GateSpec::HumanApproval { principal, .. } => {
                if principal.trim().is_empty() {
                    push("principal", "must be a non-empty principal");
                }
            }
            GateSpec::AgentSignoff { agents, k, .. } => {
                if agents.is_empty() || agents.iter().any(|a| a.trim().is_empty()) {
                    push(
                        "agents",
                        "must be a non-empty list of non-empty agent names",
                    );
                }
                let distinct: std::collections::BTreeSet<_> = agents.iter().collect();
                if distinct.len() != agents.len() {
                    push("agents", "must not contain duplicates");
                }
                if *k == 0 {
                    push("k", "must be at least 1");
                } else if *k as usize > agents.len() {
                    push("k", "cannot exceed the number of named agents");
                }
            }
            GateSpec::MachineCheck {
                check,
                max_age_seconds,
            } => {
                if check.trim().is_empty() {
                    push("check", "must be a non-empty check name");
                }
                if max_age_seconds == &Some(0) {
                    push("max_age_seconds", "must be at least 1 when present");
                }
            }
            GateSpec::PaymentEvidence => {}
            GateSpec::Timelock { min_seconds } => {
                if *min_seconds == 0 {
                    push("min_seconds", "must be at least 1");
                }
            }
        }
    }
}

/// A project's procedural law: edge key (`FROM->TO` or `any->TO`) → ordered
/// gates. Negotiated in the Quote, hash-committed at FUNDED
/// (`studio-core::gate::commitment_hash`) — weakening it mid-engagement is
/// impossible by construction.
///
/// `BTreeMap` keeps serialization order-independent of authoring order, so
/// the commitment hash is deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "Gate policy",
    description = "A project's procedural law (PLAN.md §2.1): state-machine edge → ordered hard gates. Negotiated in the Quote, hash-committed at FUNDED, tamper-evident thereafter. Semantics enforced by the engine, not expressible here: fail-closed evaluation, denial-with-note as recorded history, loud audited overrides. The implementation is additionally stricter than this schema: self-loop edge keys are rejected, agent lists must be duplicate-free, and k must not exceed the number of named agents."
)]
pub struct GatePolicy {
    /// Edge key `FROM->TO` (or wildcard `any->TO`) → ordered gates. A
    /// concrete edge is guarded by its exact entry followed by any matching
    /// wildcard entry — both apply.
    #[schemars(schema_with = "edges_schema")]
    pub edges: BTreeMap<String, Vec<GateSpec>>,
}

/// JSON Schema for `edges`: `patternProperties` keyed by the state-name
/// pattern (built from `ProjectState::ALL`, so it cannot drift from the
/// enum), everything else rejected. A plain map schema would silently accept
/// unknown edge keys the code refuses.
fn edges_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let states = ProjectState::ALL.map(|s| s.name()).join("|");
    let gates = generator.subschema_for::<Vec<GateSpec>>();
    let mut schema = serde_json::json!({
        "type": "object",
        "patternProperties": {},
        "additionalProperties": false,
    });
    schema["patternProperties"][format!("^(any|{states})->({states})$")] =
        serde_json::to_value(gates).expect("subschema serializes");
    schemars::Schema::try_from(schema).expect("edges schema is a valid JSON Schema")
}

impl GatePolicy {
    /// The studio default (PLAN.md §2.1); buyers may strengthen per-project
    /// in the Quote. Buyer approvals escalate after 48h — the same window as
    /// the session grace period — so a stalled gate is never silent.
    pub fn studio_default() -> GatePolicy {
        const ESCALATE_48H: Option<u64> = Some(172_800);
        GatePolicy {
            edges: BTreeMap::from([
                ("QUOTED->FUNDED".into(), vec![GateSpec::PaymentEvidence]),
                (
                    "DEMOED->ACCEPTED".into(),
                    vec![GateSpec::HumanApproval {
                        principal: "buyer".into(),
                        escalate_after_seconds: ESCALATE_48H,
                    }],
                ),
                (
                    "ACCEPTED->DELIVERED".into(),
                    vec![
                        GateSpec::MachineCheck {
                            check: "endpoint-live".into(),
                            max_age_seconds: None,
                        },
                        GateSpec::HumanApproval {
                            principal: "buyer".into(),
                            escalate_after_seconds: ESCALATE_48H,
                        },
                        GateSpec::Timelock {
                            min_seconds: 86_400,
                        },
                    ],
                ),
                (
                    "any->CLOSED_BY_BUYER".into(),
                    vec![GateSpec::Timelock {
                        min_seconds: 172_800,
                    }],
                ),
            ]),
        }
    }

    /// Structural validation. `field` prefixes error paths (e.g.
    /// `gate_policy` when embedded in a quote).
    pub fn validate(&self, field: &str) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();
        for (key, gates) in &self.edges {
            if let Err(message) = EdgePattern::parse(key) {
                errors.push(FieldError {
                    field: format!("{field}.edges[{key}]"),
                    message,
                });
                continue;
            }
            for (i, gate) in gates.iter().enumerate() {
                gate.validate(&format!("{field}.edges[{key}][{i}]"), &mut errors);
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

    #[test]
    fn default_policy_is_valid() {
        let p = GatePolicy::studio_default();
        assert!(p.validate("gate_policy").is_ok());
        assert_eq!(p.edges.len(), 4);
    }

    #[test]
    fn invalid_specs_are_rejected_with_precise_paths() {
        fn signoff(agents: &[&str], k: u32) -> GateSpec {
            GateSpec::AgentSignoff {
                agents: agents.iter().map(|a| a.to_string()).collect(),
                k,
                escalate_after_seconds: None,
            }
        }
        let cases: Vec<(GateSpec, &str)> = vec![
            (
                GateSpec::HumanApproval {
                    principal: "  ".into(),
                    escalate_after_seconds: None,
                },
                "principal",
            ),
            (signoff(&[], 1), "agents"),
            (signoff(&["a", "a"], 1), "agents"),
            (signoff(&["a", "b"], 0), "k"),
            (signoff(&["a", "b"], 3), "k"),
            (
                GateSpec::MachineCheck {
                    check: "".into(),
                    max_age_seconds: None,
                },
                "check",
            ),
            (
                GateSpec::MachineCheck {
                    check: "ci".into(),
                    max_age_seconds: Some(0),
                },
                "max_age_seconds",
            ),
            (GateSpec::Timelock { min_seconds: 0 }, "min_seconds"),
        ];
        for (spec, field_suffix) in cases {
            let p = GatePolicy {
                edges: BTreeMap::from([("QUOTED->FUNDED".to_string(), vec![spec.clone()])]),
            };
            let errors = p.validate("gate_policy").unwrap_err();
            assert!(
                errors.iter().any(|e| e.field.ends_with(field_suffix)
                    && e.field.starts_with("gate_policy.edges[QUOTED->FUNDED][0]")),
                "{spec:?} should fail on {field_suffix}, got {errors:?}"
            );
        }
    }

    #[test]
    fn bad_edge_keys_are_rejected() {
        for bad in ["QUOTED", "NOPE->FUNDED", "QUOTED->QUOTED", "any->any"] {
            let p = GatePolicy {
                edges: BTreeMap::from([(bad.to_string(), vec![GateSpec::PaymentEvidence])]),
            };
            assert!(p.validate("gate_policy").is_err(), "should reject `{bad}`");
        }
    }

    #[test]
    fn policy_json_round_trips_and_rejects_unknown_fields() {
        let p = GatePolicy::studio_default();
        let json = serde_json::to_value(&p).unwrap();
        let back: GatePolicy = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(p, back);

        // internally tagged spec shape, as schemas/gate-policy.json documents
        assert_eq!(
            json["edges"]["QUOTED->FUNDED"][0],
            serde_json::json!({ "type": "payment_evidence" })
        );

        for bad in [
            serde_json::json!({ "edges": {}, "surprise": 1 }),
            serde_json::json!({ "edges": { "QUOTED->FUNDED": [
                { "type": "timelock", "min_seconds": 5, "surprise": 1 }
            ]}}),
            // cross-variant field bleed must be rejected too
            serde_json::json!({ "edges": { "QUOTED->FUNDED": [
                { "type": "timelock", "min_seconds": 5, "principal": "buyer" }
            ]}}),
            serde_json::json!({ "edges": { "QUOTED->FUNDED": [
                { "type": "not_a_gate" }
            ]}}),
        ] {
            assert!(
                serde_json::from_value::<GatePolicy>(bad.clone()).is_err(),
                "should reject {bad}"
            );
        }
    }
}
