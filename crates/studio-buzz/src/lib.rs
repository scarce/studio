//! `BuzzPort` — the studio's hands on the coordination substrate.
//!
//! Trait + crate-backed impl (buzz-sdk / buzz-core / buzz-ws-client /
//! buzz-workflow, per PLAN.md §1) + mock, built in M3. The orchestrator only
//! ever sees the trait, so the state machine is testable without a relay.
//!
//! M0 ships the empty shell so the workspace shape is fixed from the first
//! commit.
