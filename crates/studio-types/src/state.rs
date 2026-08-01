//! Project states and edges (PLAN.md §2). The full transition machine (which
//! edges are legal, what evidence each records) arrives with the orchestrator
//! in M3; the gate engine needs the vocabulary now.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Every state a project can occupy (PLAN.md §2). Wire names are
/// SCREAMING_SNAKE_CASE, matching the plan's diagrams and the gate-policy
/// edge keys.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectState {
    RfqCaptured,
    Quoted,
    Lapsed,
    Funded,
    WorkroomActive,
    Building,
    Demoed,
    Accepted,
    Delivered,
    Operating,
    ClosedByBuyer,
    ClosedIdle,
}

impl ProjectState {
    pub const ALL: [ProjectState; 12] = [
        ProjectState::RfqCaptured,
        ProjectState::Quoted,
        ProjectState::Lapsed,
        ProjectState::Funded,
        ProjectState::WorkroomActive,
        ProjectState::Building,
        ProjectState::Demoed,
        ProjectState::Accepted,
        ProjectState::Delivered,
        ProjectState::Operating,
        ProjectState::ClosedByBuyer,
        ProjectState::ClosedIdle,
    ];

    /// The wire / edge-key name, e.g. `WORKROOM_ACTIVE`.
    pub fn name(self) -> &'static str {
        match self {
            ProjectState::RfqCaptured => "RFQ_CAPTURED",
            ProjectState::Quoted => "QUOTED",
            ProjectState::Lapsed => "LAPSED",
            ProjectState::Funded => "FUNDED",
            ProjectState::WorkroomActive => "WORKROOM_ACTIVE",
            ProjectState::Building => "BUILDING",
            ProjectState::Demoed => "DEMOED",
            ProjectState::Accepted => "ACCEPTED",
            ProjectState::Delivered => "DELIVERED",
            ProjectState::Operating => "OPERATING",
            ProjectState::ClosedByBuyer => "CLOSED_BY_BUYER",
            ProjectState::ClosedIdle => "CLOSED_IDLE",
        }
    }

    pub fn parse(name: &str) -> Option<ProjectState> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }
}

impl std::fmt::Display for ProjectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A concrete state-machine edge — what the orchestrator attempts and the
/// gate engine guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from: ProjectState,
    pub to: ProjectState,
}

impl std::fmt::Display for Edge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}->{}", self.from, self.to)
    }
}

/// A gate-policy edge key: either a concrete edge or an `any->TO` wildcard
/// (PLAN.md §2.1 default policy uses `any->CLOSED_BY_BUYER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgePattern {
    Exact(Edge),
    AnyTo(ProjectState),
}

impl EdgePattern {
    /// Parse an edge key like `QUOTED->FUNDED` or `any->CLOSED_BY_BUYER`.
    /// Self-loops and `any->any` are rejected: a gate on an impossible edge
    /// is a policy bug, and policy bugs must be loud (fail-closed).
    pub fn parse(key: &str) -> Result<EdgePattern, String> {
        let (from, to) = key
            .split_once("->")
            .ok_or_else(|| format!("edge key `{key}` must be `FROM->TO`"))?;
        let to = ProjectState::parse(to)
            .ok_or_else(|| format!("edge key `{key}`: unknown target state `{to}`"))?;
        if from == "any" {
            return Ok(EdgePattern::AnyTo(to));
        }
        let from = ProjectState::parse(from)
            .ok_or_else(|| format!("edge key `{key}`: unknown source state `{from}`"))?;
        if from == to {
            return Err(format!("edge key `{key}` is a self-loop"));
        }
        Ok(EdgePattern::Exact(Edge { from, to }))
    }

    pub fn matches(self, edge: Edge) -> bool {
        match self {
            EdgePattern::Exact(e) => e == edge,
            EdgePattern::AnyTo(to) => to == edge.to,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_names_round_trip() {
        for state in ProjectState::ALL {
            assert_eq!(ProjectState::parse(state.name()), Some(state));
            let json = serde_json::to_value(state).unwrap();
            assert_eq!(json, serde_json::Value::String(state.name().into()));
        }
        assert_eq!(ProjectState::parse("NOPE"), None);
    }

    #[test]
    fn edge_patterns_parse() {
        assert_eq!(
            EdgePattern::parse("QUOTED->FUNDED").unwrap(),
            EdgePattern::Exact(Edge {
                from: ProjectState::Quoted,
                to: ProjectState::Funded
            })
        );
        assert_eq!(
            EdgePattern::parse("any->CLOSED_BY_BUYER").unwrap(),
            EdgePattern::AnyTo(ProjectState::ClosedByBuyer)
        );
        for bad in [
            "QUOTED",
            "QUOTED->",
            "->FUNDED",
            "NOPE->FUNDED",
            "QUOTED->NOPE",
            "QUOTED->QUOTED",
            "any->any",
        ] {
            assert!(EdgePattern::parse(bad).is_err(), "should reject `{bad}`");
        }
    }

    #[test]
    fn wildcard_matches_only_target() {
        let close = EdgePattern::parse("any->CLOSED_BY_BUYER").unwrap();
        let from_active = Edge {
            from: ProjectState::WorkroomActive,
            to: ProjectState::ClosedByBuyer,
        };
        let from_building = Edge {
            from: ProjectState::Building,
            to: ProjectState::ClosedByBuyer,
        };
        let unrelated = Edge {
            from: ProjectState::Quoted,
            to: ProjectState::Funded,
        };
        assert!(close.matches(from_active));
        assert!(close.matches(from_building));
        assert!(!close.matches(unrelated));
    }
}
