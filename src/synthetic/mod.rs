//! Synthetic weekly credit quota from the regenerating `/v2/quotas` budget.

pub mod fetch;
pub mod types;
pub mod vendor;

pub use fetch::{FetchOutcome, fetch_snapshot};
