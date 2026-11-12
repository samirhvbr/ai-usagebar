//! Muse Code subscription quota through the official CLI's login response.
//! Credentials are read only. Returned inference keys and account fields are
//! skipped by serde, never retained in the quota cache or error messages.
pub mod creds;
pub mod fetch;
pub mod types;
pub mod vendor;
