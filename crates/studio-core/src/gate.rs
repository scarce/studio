//! Gate engine (PLAN.md §2.1). Evidence says a transition *happened*; gates
//! say whether it is *allowed to happen*. The policy wire types live in
//! `studio-types::gate`; this module owns evaluation and the commitment
//! hash.
//!
//! `eval` is a pure function: the orchestrator collects evidence and asks;
//! it never decides. Hard semantics, in order of precedence per gate:
//!
//! 1. A valid **operator override** satisfies its gate — loudly: the outcome
//!    names the operator and reason, and the override event stays in the
//!    evidence trail forever. It is the only escape hatch.
//! 2. An explicit **denial** is a first-class outcome, not an absence — it
//!    terminates the attempt (the caller rolls back and requires fresh
//!    evidence for the next attempt).
//! 3. Everything else is **fail-closed**: missing, ambiguous, or expired
//!    evidence blocks, and the block says exactly what is missing so a
//!    blocked project is self-explanatory.
//!
//! Evidence sets are scoped to one edge *attempt*. After a denial rolls a
//! milestone back, the next attempt starts with a fresh set — which is why a
//! set containing both an approval and a denial from the same principal is
//! ambiguous (fail-closed), not last-write-wins.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use studio_types::{Edge, EdgePattern, GatePolicy, GateSpec};

/// The gates guarding a concrete edge: the exact `FROM->TO` entry first,
/// then any `any->TO` entry — both apply (a wildcard strengthens, never
/// replaces). Gate indexes in outcomes and overrides refer to positions
/// in this concatenated list. An edge with no entry has no gates: gates
/// are opt-in per policy; evidence requirements still apply at the
/// state-machine layer.
pub fn gates_for(policy: &GatePolicy, edge: Edge) -> Vec<&GateSpec> {
    let mut exact = Vec::new();
    let mut wildcard = Vec::new();
    for (key, gates) in &policy.edges {
        // Keys are validated before a policy is accepted; an unparseable
        // key at eval time is treated as matching nothing (fail-closed:
        // it cannot silently gate or un-gate an edge it doesn't name).
        match EdgePattern::parse(key) {
            Ok(EdgePattern::Exact(e)) if e == edge => exact.extend(gates),
            Ok(EdgePattern::AnyTo(to)) if to == edge.to => wildcard.extend(gates),
            _ => {}
        }
    }
    exact.extend(wildcard);
    exact
}

/// SHA-256 over the policy's canonical JSON serialization (BTreeMap key
/// order, fixed field order), hex-encoded. Recorded at the FUNDED
/// transition; any later policy must hash identically or carry both
/// parties' signed consent (PLAN.md §2.1 semantics — enforcement wires in
/// with M3).
pub fn commitment_hash(policy: &GatePolicy) -> String {
    let canonical = serde_json::to_vec(policy).expect("GatePolicy serializes");
    let digest = Sha256::digest(&canonical);
    digest.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        write!(s, "{b:02x}").expect("writing to a String cannot fail");
        s
    })
}

/// One piece of collected evidence. Every variant cites its substrate
/// reference (`evidence_ref`: signed event id, approval token id, or tx
/// signature) — the engine never accepts an uncited fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GateEvidence {
    /// An approval or denial from a principal (workflow token resolution or
    /// signed channel message).
    HumanDecision {
        principal: String,
        approved: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        evidence_ref: String,
        at: DateTime<Utc>,
    },
    /// A signed sign-off message from a crew agent.
    AgentSignoff {
        agent: String,
        evidence_ref: String,
        at: DateTime<Utc>,
    },
    /// A machine-check run result. `evidence_ref` is the CI run URL, probe
    /// transcript pointer, etc.
    MachineCheck {
        check: String,
        passed: bool,
        evidence_ref: String,
        at: DateTime<Utc>,
    },
    /// `PayPort` funding/acceptance evidence (operator record or tx sig).
    Payment {
        evidence_ref: String,
        at: DateTime<Utc>,
    },
    /// The loud escape hatch: authenticated, reason-required, permanently
    /// visible. Addresses one gate by its index on the attempted edge.
    OperatorOverride {
        gate: usize,
        operator: String,
        reason: String,
        evidence_ref: String,
        at: DateTime<Utc>,
    },
}

/// Everything the orchestrator has collected for one edge attempt.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvidenceSet {
    /// When the edge became attemptable (e.g. the demo was posted). Anchors
    /// timelocks and escalation windows; without it both fail closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eligible_since: Option<DateTime<Utc>>,
    #[serde(default)]
    pub items: Vec<GateEvidence>,
}

/// How one gate was satisfied — kept loud so `GET /projects/{id}` can show
/// exactly what let a transition through, override included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "how", rename_all = "snake_case")]
pub enum GateSatisfaction {
    Evidence {
        gate: usize,
        refs: Vec<String>,
    },
    Overridden {
        gate: usize,
        operator: String,
        reason: String,
        evidence_ref: String,
    },
    Elapsed {
        gate: usize,
        eligible_since: DateTime<Utc>,
        min_seconds: u64,
    },
}

/// One unsatisfied gate, self-explanatory by contract: a blocked project must
/// tell the caller exactly what is missing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GateBlock {
    pub gate: usize,
    pub spec: GateSpec,
    pub why: String,
    /// Blocked past the gate's escalation window — notify the principal,
    /// then the studio owner. Never auto-passes.
    pub escalate: bool,
}

/// The engine's verdict on one edge attempt.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum GateOutcome {
    /// Every gate satisfied; per-gate account of how.
    Pass { satisfied: Vec<GateSatisfaction> },
    /// Fail-closed: at least one gate lacks evidence.
    Blocked {
        missing: Vec<GateBlock>,
        satisfied: Vec<GateSatisfaction>,
    },
    /// A principal explicitly denied. Terminal for this attempt: the caller
    /// records the denial as history, rolls back, and requires fresh
    /// evidence to retry.
    Denied {
        gate: usize,
        principal: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        evidence_ref: String,
    },
}

enum SingleGate {
    Satisfied(GateSatisfaction),
    Missing {
        why: String,
    },
    Denied {
        principal: String,
        note: Option<String>,
        evidence_ref: String,
    },
}

/// Evaluate one edge attempt. Pure: same inputs, same verdict.
pub fn eval(
    policy: &GatePolicy,
    edge: Edge,
    evidence: &EvidenceSet,
    now: DateTime<Utc>,
) -> GateOutcome {
    let gates = gates_for(policy, edge);
    let mut satisfied = Vec::new();
    let mut missing = Vec::new();

    for (index, spec) in gates.iter().enumerate() {
        if let Some(s) = valid_override(evidence, index) {
            satisfied.push(s);
            continue;
        }
        match eval_gate(spec, index, evidence, now) {
            SingleGate::Satisfied(s) => satisfied.push(s),
            SingleGate::Denied {
                principal,
                note,
                evidence_ref,
            } => {
                return GateOutcome::Denied {
                    gate: index,
                    principal,
                    note,
                    evidence_ref,
                }
            }
            SingleGate::Missing { why } => missing.push(GateBlock {
                gate: index,
                spec: (*spec).clone(),
                why,
                escalate: escalation_due(spec, evidence, now),
            }),
        }
    }

    if missing.is_empty() {
        GateOutcome::Pass { satisfied }
    } else {
        GateOutcome::Blocked { missing, satisfied }
    }
}

/// An override satisfies its gate only when fully formed: right gate index,
/// named operator, non-empty reason, cited event. A half-formed override is
/// ignored — it cannot quietly pass a gate.
fn valid_override(evidence: &EvidenceSet, index: usize) -> Option<GateSatisfaction> {
    evidence.items.iter().find_map(|item| match item {
        GateEvidence::OperatorOverride {
            gate,
            operator,
            reason,
            evidence_ref,
            ..
        } if *gate == index
            && !operator.trim().is_empty()
            && !reason.trim().is_empty()
            && !evidence_ref.trim().is_empty() =>
        {
            Some(GateSatisfaction::Overridden {
                gate: index,
                operator: operator.clone(),
                reason: reason.clone(),
                evidence_ref: evidence_ref.clone(),
            })
        }
        _ => None,
    })
}

fn escalation_due(spec: &GateSpec, evidence: &EvidenceSet, now: DateTime<Utc>) -> bool {
    match (spec.escalate_after_seconds(), evidence.eligible_since) {
        (Some(window), Some(since)) => {
            now.signed_duration_since(since).num_seconds() >= window as i64
        }
        _ => false,
    }
}

fn eval_gate(
    spec: &GateSpec,
    index: usize,
    evidence: &EvidenceSet,
    now: DateTime<Utc>,
) -> SingleGate {
    match spec {
        GateSpec::HumanApproval { principal, .. } => {
            let decisions: Vec<_> = evidence
                .items
                .iter()
                .filter_map(|item| match item {
                    GateEvidence::HumanDecision {
                        principal: p,
                        approved,
                        note,
                        evidence_ref,
                        at,
                    } if p == principal => Some((*approved, note, evidence_ref, at)),
                    _ => None,
                })
                .collect();
            let approvals: Vec<_> = decisions.iter().filter(|(a, ..)| *a).collect();
            let denials: Vec<_> = decisions.iter().filter(|(a, ..)| !*a).collect();
            match (approvals.is_empty(), denials.is_empty()) {
                (true, true) => SingleGate::Missing {
                    why: format!("awaiting approval from `{principal}`"),
                },
                (false, false) => SingleGate::Missing {
                    why: format!(
                        "ambiguous: `{principal}` has both an approval and a denial in \
                         this attempt — fail-closed"
                    ),
                },
                (false, true) => SingleGate::Satisfied(GateSatisfaction::Evidence {
                    gate: index,
                    refs: approvals.iter().map(|(.., r, _)| (*r).clone()).collect(),
                }),
                (true, false) => {
                    // Latest denial carries the note the caller records.
                    let (_, note, evidence_ref, _) = denials
                        .iter()
                        .max_by_key(|(.., at)| **at)
                        .expect("denials is non-empty");
                    SingleGate::Denied {
                        principal: principal.clone(),
                        note: (*note).clone(),
                        evidence_ref: (*evidence_ref).clone(),
                    }
                }
            }
        }
        GateSpec::AgentSignoff { agents, k, .. } => {
            let mut refs = Vec::new();
            let mut signers = std::collections::BTreeSet::new();
            for item in &evidence.items {
                if let GateEvidence::AgentSignoff {
                    agent,
                    evidence_ref,
                    ..
                } = item
                {
                    // Sign-offs from agents the gate does not name are
                    // ignored; the same agent counts once.
                    if agents.contains(agent) && signers.insert(agent.clone()) {
                        refs.push(evidence_ref.clone());
                    }
                }
            }
            if signers.len() >= *k as usize {
                SingleGate::Satisfied(GateSatisfaction::Evidence { gate: index, refs })
            } else {
                SingleGate::Missing {
                    why: format!(
                        "agent sign-off {}/{} (named: {})",
                        signers.len(),
                        k,
                        agents.join(", ")
                    ),
                }
            }
        }
        GateSpec::MachineCheck {
            check,
            max_age_seconds,
        } => {
            let latest = evidence
                .items
                .iter()
                .filter_map(|item| match item {
                    GateEvidence::MachineCheck {
                        check: c,
                        passed,
                        evidence_ref,
                        at,
                    } if c == check => Some((*passed, evidence_ref, *at)),
                    _ => None,
                })
                .max_by_key(|(.., at)| *at);
            match latest {
                None => SingleGate::Missing {
                    why: format!("machine check `{check}` has not run"),
                },
                Some((false, evidence_ref, _)) => SingleGate::Missing {
                    why: format!("machine check `{check}` failed (see {evidence_ref})"),
                },
                Some((true, evidence_ref, at)) => {
                    let age = now.signed_duration_since(at).num_seconds();
                    match max_age_seconds {
                        Some(max) if age > *max as i64 => SingleGate::Missing {
                            why: format!(
                                "machine check `{check}` evidence expired \
                                 ({age}s old, max {max}s) — rerun it"
                            ),
                        },
                        _ => SingleGate::Satisfied(GateSatisfaction::Evidence {
                            gate: index,
                            refs: vec![evidence_ref.clone()],
                        }),
                    }
                }
            }
        }
        GateSpec::PaymentEvidence => {
            let refs: Vec<_> = evidence
                .items
                .iter()
                .filter_map(|item| match item {
                    GateEvidence::Payment { evidence_ref, .. } => Some(evidence_ref.clone()),
                    _ => None,
                })
                .collect();
            if refs.is_empty() {
                SingleGate::Missing {
                    why: "awaiting payment evidence from PayPort".into(),
                }
            } else {
                SingleGate::Satisfied(GateSatisfaction::Evidence { gate: index, refs })
            }
        }
        GateSpec::Timelock { min_seconds } => match evidence.eligible_since {
            None => SingleGate::Missing {
                why: "timelock: edge eligibility timestamp missing — fail-closed".into(),
            },
            Some(since) => {
                let elapsed = now.signed_duration_since(since).num_seconds();
                if elapsed >= *min_seconds as i64 {
                    SingleGate::Satisfied(GateSatisfaction::Elapsed {
                        gate: index,
                        eligible_since: since,
                        min_seconds: *min_seconds,
                    })
                } else {
                    SingleGate::Missing {
                        why: format!(
                            "timelock: {}s of {min_seconds}s review window remain",
                            *min_seconds as i64 - elapsed
                        ),
                    }
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use studio_types::ProjectState;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    const NOW: &str = "2026-08-01T18:00:00Z";

    fn edge(from: ProjectState, to: ProjectState) -> Edge {
        Edge { from, to }
    }

    fn demoed_accepted() -> Edge {
        edge(ProjectState::Demoed, ProjectState::Accepted)
    }

    fn policy(key: &str, gates: Vec<GateSpec>) -> GatePolicy {
        GatePolicy {
            edges: BTreeMap::from([(key.to_string(), gates)]),
        }
    }

    fn approval(principal: &str) -> GateSpec {
        GateSpec::HumanApproval {
            principal: principal.into(),
            escalate_after_seconds: None,
        }
    }

    fn decision(principal: &str, approved: bool, note: Option<&str>, at: &str) -> GateEvidence {
        GateEvidence::HumanDecision {
            principal: principal.into(),
            approved,
            note: note.map(String::from),
            evidence_ref: format!("event:{principal}:{at}"),
            at: ts(at),
        }
    }

    fn with_items(items: Vec<GateEvidence>) -> EvidenceSet {
        EvidenceSet {
            eligible_since: Some(ts("2026-08-01T12:00:00Z")),
            items,
        }
    }

    fn eval_now(policy: &GatePolicy, edge: Edge, evidence: &EvidenceSet) -> GateOutcome {
        eval(policy, edge, evidence, ts(NOW))
    }

    // ── ungated edges ────────────────────────────────────────────────────

    #[test]
    fn edge_with_no_policy_entry_passes_with_no_gates() {
        let p = policy("QUOTED->FUNDED", vec![GateSpec::PaymentEvidence]);
        let outcome = eval_now(
            &p,
            edge(ProjectState::Funded, ProjectState::WorkroomActive),
            &EvidenceSet::default(),
        );
        assert_eq!(outcome, GateOutcome::Pass { satisfied: vec![] });
    }

    // ── payment_evidence ─────────────────────────────────────────────────

    #[test]
    fn payment_evidence_blocks_without_payment_and_passes_with_it() {
        let p = policy("QUOTED->FUNDED", vec![GateSpec::PaymentEvidence]);
        let e = edge(ProjectState::Quoted, ProjectState::Funded);

        match eval_now(&p, e, &EvidenceSet::default()) {
            GateOutcome::Blocked { missing, satisfied } => {
                assert!(satisfied.is_empty());
                assert_eq!(missing.len(), 1);
                assert_eq!(missing[0].gate, 0);
                assert!(missing[0].why.contains("payment"), "{}", missing[0].why);
                assert!(!missing[0].escalate);
            }
            other => panic!("expected Blocked, got {other:?}"),
        }

        let funded = with_items(vec![GateEvidence::Payment {
            evidence_ref: "tx:5oo…sig".into(),
            at: ts("2026-08-01T13:00:00Z"),
        }]);
        assert_eq!(
            eval_now(&p, e, &funded),
            GateOutcome::Pass {
                satisfied: vec![GateSatisfaction::Evidence {
                    gate: 0,
                    refs: vec!["tx:5oo…sig".into()]
                }]
            }
        );
    }

    // ── human_approval ───────────────────────────────────────────────────

    #[test]
    fn approval_missing_blocks_and_names_the_principal() {
        let p = policy("DEMOED->ACCEPTED", vec![approval("buyer")]);
        match eval_now(&p, demoed_accepted(), &with_items(vec![])) {
            GateOutcome::Blocked { missing, .. } => {
                assert!(missing[0].why.contains("`buyer`"), "{}", missing[0].why);
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn approval_from_the_wrong_principal_does_not_count() {
        let p = policy("DEMOED->ACCEPTED", vec![approval("buyer")]);
        let evidence = with_items(vec![decision("intern", true, None, "2026-08-01T13:00:00Z")]);
        assert!(matches!(
            eval_now(&p, demoed_accepted(), &evidence),
            GateOutcome::Blocked { .. }
        ));
    }

    #[test]
    fn approval_passes_and_cites_its_event() {
        let p = policy("DEMOED->ACCEPTED", vec![approval("buyer")]);
        let evidence = with_items(vec![decision("buyer", true, None, "2026-08-01T13:00:00Z")]);
        match eval_now(&p, demoed_accepted(), &evidence) {
            GateOutcome::Pass { satisfied } => {
                assert_eq!(
                    satisfied,
                    vec![GateSatisfaction::Evidence {
                        gate: 0,
                        refs: vec!["event:buyer:2026-08-01T13:00:00Z".into()]
                    }]
                );
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn denial_is_a_first_class_outcome_with_the_latest_note() {
        let p = policy("DEMOED->ACCEPTED", vec![approval("buyer")]);
        let evidence = with_items(vec![
            decision("buyer", false, Some("first pass"), "2026-08-01T13:00:00Z"),
            decision(
                "buyer",
                false,
                Some("latency still 4s"),
                "2026-08-01T14:00:00Z",
            ),
        ]);
        assert_eq!(
            eval_now(&p, demoed_accepted(), &evidence),
            GateOutcome::Denied {
                gate: 0,
                principal: "buyer".into(),
                note: Some("latency still 4s".into()),
                evidence_ref: "event:buyer:2026-08-01T14:00:00Z".into(),
            }
        );
    }

    #[test]
    fn conflicting_approval_and_denial_are_ambiguous_and_block() {
        let p = policy("DEMOED->ACCEPTED", vec![approval("buyer")]);
        let evidence = with_items(vec![
            decision("buyer", true, None, "2026-08-01T13:00:00Z"),
            decision(
                "buyer",
                false,
                Some("changed my mind"),
                "2026-08-01T14:00:00Z",
            ),
        ]);
        match eval_now(&p, demoed_accepted(), &evidence) {
            GateOutcome::Blocked { missing, .. } => {
                assert!(missing[0].why.contains("ambiguous"), "{}", missing[0].why);
            }
            other => panic!("expected Blocked (fail-closed), got {other:?}"),
        }
    }

    #[test]
    fn denial_takes_precedence_over_other_missing_gates() {
        let p = policy(
            "DEMOED->ACCEPTED",
            vec![GateSpec::PaymentEvidence, approval("buyer")],
        );
        let evidence = with_items(vec![decision(
            "buyer",
            false,
            Some("not what we agreed"),
            "2026-08-01T13:00:00Z",
        )]);
        assert!(matches!(
            eval_now(&p, demoed_accepted(), &evidence),
            GateOutcome::Denied { gate: 1, .. }
        ));
    }

    // ── agent_signoff ────────────────────────────────────────────────────

    fn signoff_gate(agents: &[&str], k: u32) -> GateSpec {
        GateSpec::AgentSignoff {
            agents: agents.iter().map(|a| a.to_string()).collect(),
            k,
            escalate_after_seconds: None,
        }
    }

    fn signoff(agent: &str, r: &str) -> GateEvidence {
        GateEvidence::AgentSignoff {
            agent: agent.into(),
            evidence_ref: r.into(),
            at: ts("2026-08-01T13:00:00Z"),
        }
    }

    #[test]
    fn signoff_k_of_n_counts_distinct_named_agents_only() {
        let p = policy(
            "DEMOED->ACCEPTED",
            vec![signoff_gate(&["reviewer", "qa"], 2)],
        );
        let e = demoed_accepted();

        // 0 of 2
        match eval_now(&p, e, &with_items(vec![])) {
            GateOutcome::Blocked { missing, .. } => {
                assert!(missing[0].why.contains("0/2"), "{}", missing[0].why)
            }
            other => panic!("expected Blocked, got {other:?}"),
        }

        // duplicate signer counts once; outsider never counts
        let evidence = with_items(vec![
            signoff("reviewer", "ev1"),
            signoff("reviewer", "ev2"),
            signoff("stranger", "ev3"),
        ]);
        match eval_now(&p, e, &evidence) {
            GateOutcome::Blocked { missing, .. } => {
                assert!(missing[0].why.contains("1/2"), "{}", missing[0].why)
            }
            other => panic!("expected Blocked, got {other:?}"),
        }

        // 2 of 2 passes with both refs
        let evidence = with_items(vec![signoff("reviewer", "ev1"), signoff("qa", "ev4")]);
        match eval_now(&p, e, &evidence) {
            GateOutcome::Pass { satisfied } => match &satisfied[0] {
                GateSatisfaction::Evidence { refs, .. } => {
                    assert_eq!(refs, &vec!["ev1".to_string(), "ev4".to_string()])
                }
                other => panic!("expected Evidence, got {other:?}"),
            },
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn signoff_one_of_two_needs_only_one() {
        let p = policy(
            "DEMOED->ACCEPTED",
            vec![signoff_gate(&["reviewer", "qa"], 1)],
        );
        let evidence = with_items(vec![signoff("qa", "ev1")]);
        assert!(matches!(
            eval_now(&p, demoed_accepted(), &evidence),
            GateOutcome::Pass { .. }
        ));
    }

    // ── machine_check ────────────────────────────────────────────────────

    fn check_evidence(check: &str, passed: bool, at: &str) -> GateEvidence {
        GateEvidence::MachineCheck {
            check: check.into(),
            passed,
            evidence_ref: format!("ci:{at}"),
            at: ts(at),
        }
    }

    #[test]
    fn machine_check_missing_failed_and_rerun_semantics() {
        let p = policy(
            "ACCEPTED->DELIVERED",
            vec![GateSpec::MachineCheck {
                check: "endpoint-live".into(),
                max_age_seconds: None,
            }],
        );
        let e = edge(ProjectState::Accepted, ProjectState::Delivered);

        // never ran
        match eval_now(&p, e, &with_items(vec![])) {
            GateOutcome::Blocked { missing, .. } => {
                assert!(missing[0].why.contains("has not run"), "{}", missing[0].why)
            }
            other => panic!("expected Blocked, got {other:?}"),
        }

        // failed
        let failed = with_items(vec![check_evidence(
            "endpoint-live",
            false,
            "2026-08-01T13:00:00Z",
        )]);
        match eval_now(&p, e, &failed) {
            GateOutcome::Blocked { missing, .. } => {
                assert!(missing[0].why.contains("failed"), "{}", missing[0].why)
            }
            other => panic!("expected Blocked, got {other:?}"),
        }

        // fail then pass: latest run wins
        let recovered = with_items(vec![
            check_evidence("endpoint-live", false, "2026-08-01T13:00:00Z"),
            check_evidence("endpoint-live", true, "2026-08-01T14:00:00Z"),
        ]);
        assert!(matches!(
            eval_now(&p, e, &recovered),
            GateOutcome::Pass { .. }
        ));

        // pass then fail: latest run wins, fail-closed
        let regressed = with_items(vec![
            check_evidence("endpoint-live", true, "2026-08-01T13:00:00Z"),
            check_evidence("endpoint-live", false, "2026-08-01T14:00:00Z"),
        ]);
        assert!(matches!(
            eval_now(&p, e, &regressed),
            GateOutcome::Blocked { .. }
        ));

        // a run for a different check never counts
        let unrelated = with_items(vec![check_evidence(
            "ci-green",
            true,
            "2026-08-01T14:00:00Z",
        )]);
        assert!(matches!(
            eval_now(&p, e, &unrelated),
            GateOutcome::Blocked { .. }
        ));
    }

    #[test]
    fn stale_machine_check_evidence_expires_and_blocks() {
        let p = policy(
            "ACCEPTED->DELIVERED",
            vec![GateSpec::MachineCheck {
                check: "endpoint-live".into(),
                max_age_seconds: Some(3_600),
            }],
        );
        let e = edge(ProjectState::Accepted, ProjectState::Delivered);

        // 5h old > 1h max → expired
        let stale = with_items(vec![check_evidence(
            "endpoint-live",
            true,
            "2026-08-01T13:00:00Z",
        )]);
        match eval_now(&p, e, &stale) {
            GateOutcome::Blocked { missing, .. } => {
                assert!(missing[0].why.contains("expired"), "{}", missing[0].why)
            }
            other => panic!("expected Blocked, got {other:?}"),
        }

        // 30min old ≤ 1h max → fresh
        let fresh = with_items(vec![check_evidence(
            "endpoint-live",
            true,
            "2026-08-01T17:30:00Z",
        )]);
        assert!(matches!(eval_now(&p, e, &fresh), GateOutcome::Pass { .. }));
    }

    // ── timelock ─────────────────────────────────────────────────────────

    #[test]
    fn timelock_needs_an_eligibility_anchor_and_the_full_window() {
        let p = policy(
            "any->CLOSED_BY_BUYER",
            vec![GateSpec::Timelock {
                min_seconds: 172_800,
            }],
        );
        let e = edge(ProjectState::WorkroomActive, ProjectState::ClosedByBuyer);

        // no anchor → fail-closed
        let anchorless = EvidenceSet {
            eligible_since: None,
            items: vec![],
        };
        match eval_now(&p, e, &anchorless) {
            GateOutcome::Blocked { missing, .. } => {
                assert!(missing[0].why.contains("fail-closed"), "{}", missing[0].why)
            }
            other => panic!("expected Blocked, got {other:?}"),
        }

        // 6h elapsed of 48h → blocked with remaining time
        match eval_now(&p, e, &with_items(vec![])) {
            GateOutcome::Blocked { missing, .. } => {
                assert!(missing[0].why.contains("timelock"), "{}", missing[0].why)
            }
            other => panic!("expected Blocked, got {other:?}"),
        }

        // window elapsed → pass, citing the anchor
        let elapsed = EvidenceSet {
            eligible_since: Some(ts("2026-07-29T12:00:00Z")),
            items: vec![],
        };
        assert_eq!(
            eval_now(&p, e, &elapsed),
            GateOutcome::Pass {
                satisfied: vec![GateSatisfaction::Elapsed {
                    gate: 0,
                    eligible_since: ts("2026-07-29T12:00:00Z"),
                    min_seconds: 172_800,
                }]
            }
        );
    }

    // ── operator override ────────────────────────────────────────────────

    fn override_evidence(gate: usize, operator: &str, reason: &str) -> GateEvidence {
        GateEvidence::OperatorOverride {
            gate,
            operator: operator.into(),
            reason: reason.into(),
            evidence_ref: "event:override".into(),
            at: ts("2026-08-01T14:00:00Z"),
        }
    }

    #[test]
    fn override_satisfies_its_gate_loudly() {
        let p = policy("DEMOED->ACCEPTED", vec![approval("buyer")]);
        let evidence = with_items(vec![override_evidence(
            0,
            "ludovic",
            "buyer confirmed by phone, key ceremony pending",
        )]);
        match eval_now(&p, demoed_accepted(), &evidence) {
            GateOutcome::Pass { satisfied } => assert_eq!(
                satisfied,
                vec![GateSatisfaction::Overridden {
                    gate: 0,
                    operator: "ludovic".into(),
                    reason: "buyer confirmed by phone, key ceremony pending".into(),
                    evidence_ref: "event:override".into(),
                }]
            ),
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn override_beats_a_denial_but_stays_visible_as_an_override() {
        // The escape hatch works even against an explicit denial — but the
        // outcome says Overridden, and the denial stays in the evidence set.
        let p = policy("DEMOED->ACCEPTED", vec![approval("buyer")]);
        let evidence = with_items(vec![
            decision("buyer", false, Some("hold"), "2026-08-01T13:00:00Z"),
            override_evidence(0, "ludovic", "buyer retracted the hold out-of-band"),
        ]);
        assert!(matches!(
            eval_now(&p, demoed_accepted(), &evidence),
            GateOutcome::Pass { satisfied } if matches!(satisfied[0], GateSatisfaction::Overridden { .. })
        ));
    }

    #[test]
    fn half_formed_overrides_never_satisfy() {
        let p = policy("DEMOED->ACCEPTED", vec![approval("buyer")]);
        for bad in [
            override_evidence(0, "ludovic", "   "), // reason required
            override_evidence(0, "", "reason"),     // operator required
            override_evidence(1, "ludovic", "wrong gate index"),
            GateEvidence::OperatorOverride {
                gate: 0,
                operator: "ludovic".into(),
                reason: "uncited".into(),
                evidence_ref: "".into(), // citation required
                at: ts("2026-08-01T14:00:00Z"),
            },
        ] {
            let evidence = with_items(vec![bad.clone()]);
            assert!(
                matches!(
                    eval_now(&p, demoed_accepted(), &evidence),
                    GateOutcome::Blocked { .. }
                ),
                "should stay blocked with {bad:?}"
            );
        }
    }

    #[test]
    fn override_on_one_gate_leaves_the_others_gated() {
        let p = policy(
            "ACCEPTED->DELIVERED",
            vec![
                GateSpec::MachineCheck {
                    check: "endpoint-live".into(),
                    max_age_seconds: None,
                },
                approval("buyer"),
            ],
        );
        let e = edge(ProjectState::Accepted, ProjectState::Delivered);
        let evidence = with_items(vec![override_evidence(0, "ludovic", "probe rig is down")]);
        match eval_now(&p, e, &evidence) {
            GateOutcome::Blocked { missing, satisfied } => {
                assert_eq!(missing.len(), 1);
                assert_eq!(missing[0].gate, 1);
                assert!(matches!(
                    satisfied[0],
                    GateSatisfaction::Overridden { gate: 0, .. }
                ));
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    // ── escalation ───────────────────────────────────────────────────────

    #[test]
    fn blocked_past_the_escalation_window_escalates_never_auto_passes() {
        let p = policy(
            "DEMOED->ACCEPTED",
            vec![GateSpec::HumanApproval {
                principal: "buyer".into(),
                escalate_after_seconds: Some(3_600),
            }],
        );
        // eligible since 12:00, now 18:00 → 6h blocked, window 1h
        match eval_now(&p, demoed_accepted(), &with_items(vec![])) {
            GateOutcome::Blocked { missing, .. } => assert!(missing[0].escalate),
            other => panic!("expected Blocked, got {other:?}"),
        }

        // inside the window → no escalation yet
        let p_wide = policy(
            "DEMOED->ACCEPTED",
            vec![GateSpec::HumanApproval {
                principal: "buyer".into(),
                escalate_after_seconds: Some(86_400),
            }],
        );
        match eval_now(&p_wide, demoed_accepted(), &with_items(vec![])) {
            GateOutcome::Blocked { missing, .. } => assert!(!missing[0].escalate),
            other => panic!("expected Blocked, got {other:?}"),
        }

        // no eligibility anchor → cannot measure → no escalation flag
        let anchorless = EvidenceSet::default();
        match eval_now(&p, demoed_accepted(), &anchorless) {
            GateOutcome::Blocked { missing, .. } => assert!(!missing[0].escalate),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    // ── multi-gate edges & wildcards ─────────────────────────────────────

    #[test]
    fn blocked_lists_exactly_the_missing_gates() {
        let p = policy(
            "ACCEPTED->DELIVERED",
            vec![
                GateSpec::MachineCheck {
                    check: "endpoint-live".into(),
                    max_age_seconds: None,
                },
                approval("buyer"),
                GateSpec::Timelock { min_seconds: 60 },
            ],
        );
        let e = edge(ProjectState::Accepted, ProjectState::Delivered);
        let evidence = with_items(vec![check_evidence(
            "endpoint-live",
            true,
            "2026-08-01T17:00:00Z",
        )]);
        match eval_now(&p, e, &evidence) {
            GateOutcome::Blocked { missing, satisfied } => {
                assert_eq!(missing.iter().map(|m| m.gate).collect::<Vec<_>>(), [1]);
                // check passed, timelock elapsed (6h ≥ 60s)
                assert_eq!(satisfied.len(), 2);
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn exact_and_wildcard_entries_both_apply_in_order() {
        let p = GatePolicy {
            edges: BTreeMap::from([
                (
                    "WORKROOM_ACTIVE->CLOSED_BY_BUYER".to_string(),
                    vec![approval("buyer")],
                ),
                (
                    "any->CLOSED_BY_BUYER".to_string(),
                    vec![GateSpec::Timelock {
                        min_seconds: 172_800,
                    }],
                ),
            ]),
        };
        let e = edge(ProjectState::WorkroomActive, ProjectState::ClosedByBuyer);
        match eval_now(&p, e, &with_items(vec![])) {
            GateOutcome::Blocked { missing, .. } => {
                // gate 0 = exact entry's approval, gate 1 = wildcard timelock
                assert_eq!(missing.iter().map(|m| m.gate).collect::<Vec<_>>(), [0, 1]);
                assert!(missing[0].why.contains("buyer"));
                assert!(missing[1].why.contains("timelock"));
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    // ── default policy through the engine ────────────────────────────────

    #[test]
    fn default_policy_gates_match_the_plan() {
        let p = GatePolicy::studio_default();
        assert_eq!(
            gates_for(&p, edge(ProjectState::Quoted, ProjectState::Funded)),
            vec![&GateSpec::PaymentEvidence]
        );
        assert_eq!(
            gates_for(&p, edge(ProjectState::Accepted, ProjectState::Delivered)).len(),
            3
        );
    }

    // ── commitment hash ──────────────────────────────────────────────────

    #[test]
    fn commitment_hash_is_deterministic_and_insertion_order_independent() {
        let a = GatePolicy::studio_default();
        // Rebuild the same policy inserting edges in reverse order.
        let mut reversed = GatePolicy {
            edges: BTreeMap::new(),
        };
        for (k, v) in a.edges.iter().rev() {
            reversed.edges.insert(k.clone(), v.clone());
        }
        assert_eq!(commitment_hash(&a), commitment_hash(&reversed));
        assert_eq!(commitment_hash(&a).len(), 64);
    }

    #[test]
    fn commitment_hash_changes_when_the_policy_changes() {
        let a = GatePolicy::studio_default();
        let mut weakened = a.clone();
        weakened.edges.remove("ACCEPTED->DELIVERED");
        assert_ne!(commitment_hash(&a), commitment_hash(&weakened));

        let mut tightened = a.clone();
        tightened
            .edges
            .get_mut("QUOTED->FUNDED")
            .unwrap()
            .push(GateSpec::Timelock { min_seconds: 60 });
        assert_ne!(commitment_hash(&a), commitment_hash(&tightened));
    }
}
