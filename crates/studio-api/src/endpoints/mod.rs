//! One module per endpoint (SF API conventions). `/healthz` lives in the
//! crate root.

pub mod create_rfq;
pub mod get_rfq;
pub mod list_rfqs;
