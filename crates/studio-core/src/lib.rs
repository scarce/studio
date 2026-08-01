//! Business logic over the wire types. **No I/O.**
//!
//! Everything in this crate is a pure function over data: the gate engine
//! (PLAN.md §2.1), record assembly (RFQ capture, quote issuance), and — with
//! M3 — the full transition machine. The types themselves live in
//! `studio-types` (re-exported here); orchestration, storage, and transport
//! live in the sibling crates. This crate must stay testable without a
//! relay, a chain, or a database — and callable from any surface (HTTP, CLI,
//! MCP) without duplicating logic.

pub mod gate;
pub mod quote;
pub mod rfq;

pub use gate::{
    commitment_hash, eval, gates_for, EvidenceSet, GateBlock, GateEvidence, GateOutcome,
    GateSatisfaction,
};
pub use studio_types::{
    Amount, ChannelParams, Edge, EdgePattern, FieldError, GatePolicy, GateSpec, MilestoneSpec,
    NewQuote, NewRfq, PayoutDestination, ProjectState, Quote, QuoteStatus, Rfq, Split,
};
