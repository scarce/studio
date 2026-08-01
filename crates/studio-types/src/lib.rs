//! Wire and domain types for scarce-studio — the single source of the API
//! contract. **No I/O, no orchestration.**
//!
//! Every type here derives `serde` for the wire and `schemars::JsonSchema`
//! for discoverability: the checked-in `schemas/*.json` files are *generated*
//! from these types (see [`schemas`]), so the JSON Schema contract cannot
//! drift from the code. Field validation lives next to the types; state
//! machines and orchestration logic stay in `studio-core`.

pub mod gate;
pub mod quote;
pub mod rfq;
pub mod schemas;
pub mod state;

pub use gate::{GatePolicy, GateSpec};
pub use quote::{
    ChannelParams, MilestoneSpec, NewQuote, PayoutDestination, Quote, QuoteStatus, Split,
};
pub use rfq::{validate_npub, Amount, FieldError, NewRfq, Rfq};
pub use state::{Edge, EdgePattern, ProjectState};
