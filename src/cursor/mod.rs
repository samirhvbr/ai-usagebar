//! Cursor — included-usage pools read from the local Cursor IDE session
//! token (`state.vscdb`) against the undocumented `cursor.com/api/usage-summary`
//! endpoint the Cursor dashboard itself calls, plus spending-page credit
//! grants from `GetClientVisibleCreditGrants`. See `db.rs` for the token
//! source and `fetch.rs`/`types.rs` for the wire calls and schema.

pub mod db;
pub mod fetch;
pub mod types;
pub mod vendor;

pub use fetch::{FetchOutcome, fetch_snapshot};
