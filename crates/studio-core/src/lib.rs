//! Business logic over the wire types. **No I/O.**
//!
//! Everything in this crate is a pure function over data: the state machine
//! (PLAN.md §2), the gate engine (PLAN.md §2.1), and record assembly. The
//! types themselves live in `studio-types` (re-exported here); orchestration,
//! storage, and transport live in the sibling crates. This crate must stay
//! testable without a relay, a chain, or a database — and callable from any
//! surface (HTTP, CLI, MCP) without duplicating logic.

pub mod rfq;

pub use studio_types::{Amount, FieldError, NewRfq, Rfq};
