//! `PayPort` — funding, acceptance, and close status for a project.
//!
//! Stub impl (operator overrides + signed channel messages as evidence)
//! ships first and carries M0–M4; the live impl (session `open` / voucher
//! settlement / `settleAndSeal` observed via RPC) lands in M5 behind a
//! feature flag. The state machine cannot tell the difference — that is the
//! point (PLAN.md §1).
//!
//! M0 ships the empty shell so the workspace shape is fixed from the first
//! commit.
