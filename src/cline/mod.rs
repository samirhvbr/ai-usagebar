//! Cline (ClinePass) subscription quota integration.
//!
//! Cline computes the three ClinePass limits (5 hours, weekly, monthly) itself;
//! this vendor reads them from `usage-limits`, the same call the dashboard
//! makes. See [`types`] for why they are not recomputed from the charge ledger.

pub mod creds;
pub mod fetch;
pub mod types;
pub mod vendor;
