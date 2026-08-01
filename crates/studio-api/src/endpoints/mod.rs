//! One module per endpoint (SF API conventions). `/healthz` lives in the
//! crate root.

pub mod accept_quote;
pub mod api_index;
pub mod create_quote;
pub mod create_rfq;
pub mod get_project;
pub mod get_quote;
pub mod get_rfq;
pub mod get_schema;
pub mod list_rfqs;
