//! Domain types, schemas, and the project state machine. **No I/O.**
//!
//! Everything in this crate is a pure function over data: the state machine
//! (PLAN.md §2), the gate engine (PLAN.md §2.1), and the RFQ/Quote schemas.
//! Orchestration, storage, and transport live in the sibling crates; this
//! crate must stay testable without a relay, a chain, or a database.

pub mod rfq;

pub use rfq::{Amount, FieldError, NewRfq, Rfq};
