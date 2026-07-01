//! Anthropic vendor — OAuth-based plan usage via the undocumented
//! `https://api.anthropic.com/api/oauth/usage` endpoint.
//!
//! Mirrors `~/Projects/claudebar/claudebar` line-for-line; see individual
//! submodule headers for the bash references.

pub mod creds;
pub mod fetch;
#[cfg(target_os = "macos")]
pub mod keychain;
pub mod oauth;
pub mod types;

pub use fetch::{FetchOutcome, fetch_snapshot};

/// True when a fetch error means the OAuth session must be re-established
/// interactively — ai-usagebar shares Claude Code's single refresh token and
/// can't mint a new one itself, so the only fix is a login (run `claude` or
/// re-login in the IDE). Matched on the error *message* (code-agnostic) so the
/// same check serves a `FetchOutcome::last_error` tuple, a `TabState::Error`
/// string, and the Waybar tooltip alike:
///   - "Refresh token not found or invalid" / "Refresh token expired" (server)
///   - "token refresh failed; run `claude` to re-auth" (fetch no-cache path)
///   - "…Run `claude` to re-authenticate." (creds-parse path)
pub fn is_reauth_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("refresh token") || m.contains("invalid_grant") || m.contains("re-auth")
}

#[cfg(test)]
mod tests {
    use super::is_reauth_error;

    #[test]
    fn is_reauth_error_matches_oauth_failures_only() {
        // The messages fetch.rs/oauth.rs/creds.rs actually produce.
        assert!(is_reauth_error("Refresh token not found or invalid"));
        assert!(is_reauth_error("Refresh token expired"));
        assert!(is_reauth_error(
            "token refresh failed; run `claude` to re-auth"
        ));
        assert!(is_reauth_error(
            "could not parse …. Run `claude` to re-authenticate."
        ));
        assert!(is_reauth_error("invalid_grant"));
        // Unrelated transient/HTTP errors must NOT masquerade as re-auth.
        assert!(!is_reauth_error("slow down"));
        assert!(!is_reauth_error("rate_limit_error"));
        assert!(!is_reauth_error("Internal server error"));
    }
}
