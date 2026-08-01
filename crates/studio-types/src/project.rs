//! The public project view — what `GET /api/v1/projects/{id}` returns and
//! the embedded `/project/{id}` page renders.
//!
//! **Deliberately commercial-free.** The page is shareable (the accept
//! response hands its URL to the buyer, who may forward it); price, payout
//! splits, budget and gate policy stay inside the private workroom. This
//! type carries only what excites an outsider: what is being built, that
//! agents are on it, and how to join Buzz to watch.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::ProjectState;

/// Public projection of one engagement, keyed by RFQ id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Project {
    /// The RFQ id — also the page path segment (`/project/{id}`).
    pub id: String,
    /// The demand, verbatim — doubles as the project title.
    pub title: String,
    /// Lifecycle state (PLAN.md §2 vocabulary).
    pub state: ProjectState,
    /// When the demand was captured.
    pub created_at: DateTime<Utc>,
    /// Public slice of the quote, present once one is issued.
    pub quote: Option<ProjectQuote>,
    /// The Buzz workroom, present once the contract started.
    pub workroom: Option<ProjectWorkroom>,
    pub links: ProjectLinks,
}

/// Quote facts safe for a public page — scope and schedule, no money.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectQuote {
    pub milestones: Vec<ProjectMilestone>,
    pub timeline: String,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
}

/// Milestone scope without its amount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectMilestone {
    pub title: String,
    pub description: String,
}

/// The private Buzz channel where the crew works. Membership is required to
/// read it — the page can only point at the door.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectWorkroom {
    /// Channel display name (`proj-<slug>-<shortid>`).
    pub name: String,
    /// Channel uuid on the community relay.
    pub channel_id: String,
    /// When the workroom opened — the contract start.
    pub since: DateTime<Utc>,
}

/// Onboarding links the page renders as calls to action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectLinks {
    /// Web entry to the studio's Buzz community, when one is configured.
    pub community_web: Option<String>,
    /// Buzz Desktop download.
    pub buzz_desktop: String,
}
