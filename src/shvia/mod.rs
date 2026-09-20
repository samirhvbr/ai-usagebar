//! ShvIA vendor — a self-hosted OpenAI-compatible gateway exposing token
//! usage at `{base_url}/api/v1/usage`.
//!
//! Auth header is `Authorization: Bearer <KEY>` (WITH the `Bearer ` prefix —
//! note the Z.AI vendor deliberately omits it; ShvIA requires it).

pub mod fetch;
pub mod types;
pub mod vendor;

pub use fetch::{FetchOutcome, fetch_snapshot};
