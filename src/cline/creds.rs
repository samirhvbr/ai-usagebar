//! Locate the Cline credential the Cline CLI already holds.
//!
//! This module reads; it never writes. Cline's access token is short-lived
//! (five hours) and refreshing it rotates the refresh token, so a refresh
//! performed here would invalidate the copy `~/.cline` holds and log the
//! user's own CLI out. Refreshing belongs to the CLI that owns the file — the
//! widget reports "sign-in expired" instead, exactly as it would for any other
//! credential it does not own.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cache::home_dir;
use crate::error::{AppError, Result};

/// The Cline CLI's provider store, relative to the home directory.
pub const AUTH_FILE: &str = ".cline/data/settings/providers.json";

/// Env override, for a machine where the CLI is not installed at all. Cline
/// issues these from Settings → API Keys at app.cline.bot.
pub const API_KEY_ENV: &str = "CLINE_API_KEY";

pub const SIGNED_OUT: &str =
    "Cline is not signed in. Run `cline auth` to sign in, or set CLINE_API_KEY.";
pub const EXPIRED: &str = "Cline sign-in expired. Run `cline auth` to sign in again.";

/// A credential and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    /// Sent verbatim as the bearer token. Cline's own tokens carry a
    /// `workos:` prefix that the API requires, so nothing is stripped here.
    pub token: String,
    /// The Cline user id (`usr-…`) the token belongs to, when the store
    /// records it. Saves a `/users/me` round trip on every refresh.
    pub account_id: Option<String>,
    pub source: String,
    /// The store's own expiry has passed, so the API will reject this token.
    ///
    /// Returned rather than withheld: the account id travels with it, which
    /// is what lets the vendor serve its cached ledger — correctly aged — for
    /// the hours between two Cline sessions, instead of blanking the bar
    /// every night. Only the CLI can mint a fresh token; see the module note.
    pub expired: bool,
}

/// One candidate found on disk, before expiry is considered.
struct Candidate {
    token: String,
    account_id: Option<String>,
    expires_ms: Option<i64>,
}

/// Default search path, resolved against the OS home convention.
pub fn default_paths() -> Result<Vec<PathBuf>> {
    Ok(vec![home_dir()?.join(AUTH_FILE)])
}

/// Read whichever credential the CLI wrote into `path`.
///
/// A missing or malformed file yields `None` rather than an error so a second
/// configured path still gets its turn.
pub fn read_from(path: &Path) -> Option<Credential> {
    let text = std::fs::read_to_string(path).ok()?;
    let now = now_ms();
    best_candidate(&text, now).map(|found| Credential {
        token: found.token,
        account_id: found.account_id,
        source: path.display().to_string(),
        expired: found.expires_ms.is_some_and(|expires| expires <= now),
    })
}

/// The live credential with the most life left in it.
///
/// The store keeps one entry per configured provider (`cline`, `cline-pass`,
/// …) and they are refreshed independently, so the file routinely holds a
/// lapsed token beside a current one. Picking the furthest-from-expiry entry
/// is what keeps the widget on the copy the CLI is actually maintaining.
fn best_candidate(text: &str, now_ms: i64) -> Option<Candidate> {
    let value: Value = serde_json::from_str(text).ok()?;
    let providers = value.get("providers")?.as_object()?;
    // A live token always wins; the freshest lapsed one is the fallback, and
    // carries the account id the cache is keyed on.
    providers
        .values()
        .filter_map(candidate)
        .max_by_key(|found| {
            let expires = found.expires_ms.unwrap_or(i64::MAX);
            (expires > now_ms, expires)
        })
}

fn candidate(provider: &Value) -> Option<Candidate> {
    let auth = provider.get("settings")?.get("auth")?;
    let token = auth
        .get("accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())?
        .to_string();
    Some(Candidate {
        token,
        account_id: auth
            .get("accountId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        expires_ms: auth.get("expiresAt").and_then(Value::as_i64),
    })
}

/// Production resolver: the env override, then configured paths, then the
/// platform default. Tests use [`resolve_from`], which takes both as inputs.
pub fn resolve(
    configured: Option<&[PathBuf]>,
    env_var_name: &str,
    inline_key: Option<&str>,
) -> Result<Credential> {
    let paths = match configured {
        Some(paths) if !paths.is_empty() => paths.to_vec(),
        _ => default_paths()?,
    };
    let env_key = crate::config::optional_api_key(env_var_name, inline_key);
    resolve_from(env_key.as_deref(), &paths)
}

/// Resolve one credential from the environment, then each path in turn.
pub fn resolve_from(env_key: Option<&str>, paths: &[PathBuf]) -> Result<Credential> {
    if let Some(key) = env_key.map(str::trim).filter(|key| !key.is_empty()) {
        return Ok(Credential {
            token: key.to_string(),
            account_id: None,
            source: API_KEY_ENV.to_string(),
            expired: false,
        });
    }
    for path in paths {
        if let Some(credential) = read_from(path) {
            return Ok(credential);
        }
    }
    Err(AppError::Credentials(message_for(paths)))
}

/// Reached only when no store held a token at all — a lapsed one is returned
/// flagged rather than withheld, so "expired" is reported by the vendor with
/// the cached figures beside it.
fn message_for(_paths: &[PathBuf]) -> String {
    SIGNED_OUT.to_string()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_788_000_000_000;

    fn store(entries: &str) -> String {
        format!(r#"{{"version":1,"providers":{{{entries}}}}}"#)
    }

    fn entry(name: &str, token: &str, expires: i64) -> String {
        format!(
            r#""{name}":{{"settings":{{"auth":{{"accessToken":"{token}","accountId":"usr-1","expiresAt":{expires}}}}}}}"#
        )
    }

    #[test]
    fn picks_the_token_with_the_most_life_left() {
        let text = store(&format!(
            "{},{}",
            entry("cline-pass", "stale", NOW_MS + 60_000),
            entry("cline", "current", NOW_MS + 3_600_000)
        ));

        let found = best_candidate(&text, NOW_MS).expect("a live credential");
        assert_eq!(found.token, "current");
        assert_eq!(found.account_id.as_deref(), Some("usr-1"));
    }

    #[test]
    fn a_live_token_outranks_a_lapsed_one_even_when_the_lapsed_one_sorts_later() {
        let text = store(&format!(
            "{},{}",
            entry("cline-pass", "lapsed", NOW_MS - 1),
            entry("cline", "live", NOW_MS + 1_000)
        ));

        let found = best_candidate(&text, NOW_MS).expect("a live credential");
        assert_eq!(found.token, "live");
    }

    #[test]
    fn a_lapsed_token_is_returned_flagged_not_withheld() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("providers.json");
        std::fs::write(&path, store(&entry("cline", "lapsed", 1))).expect("write store");

        let credential = read_from(&path).expect("a lapsed credential is still a credential");
        assert!(credential.expired);
        assert_eq!(credential.account_id.as_deref(), Some("usr-1"));
    }

    #[test]
    fn keeps_the_workos_prefix_the_api_requires() {
        let text = store(&entry("cline", "workos:eyJhbGciOi", NOW_MS + 1000));
        assert_eq!(
            best_candidate(&text, NOW_MS).expect("credential").token,
            "workos:eyJhbGciOi"
        );
    }

    #[test]
    fn an_entry_without_an_expiry_is_still_usable() {
        let text = store(r#""cline":{"settings":{"auth":{"accessToken":"forever"}}}"#);
        let found = best_candidate(&text, NOW_MS).expect("credential");
        assert_eq!(found.token, "forever");
        assert!(found.account_id.is_none());
    }

    #[test]
    fn malformed_stores_are_not_credentials() {
        for text in [
            "not json".to_string(),
            "{}".to_string(),
            store(""),
            store(r#""cline":{"settings":{}}"#),
            store(r#""cline":{"settings":{"auth":{"accessToken":"  "}}}"#),
        ] {
            assert!(
                best_candidate(&text, NOW_MS).is_none(),
                "unexpectedly accepted {text}"
            );
        }
    }

    #[test]
    fn env_key_wins_over_every_path() {
        let credential = resolve_from(Some("sk-cline"), &[PathBuf::from("/nonexistent")])
            .expect("env credential");
        assert_eq!(credential.token, "sk-cline");
        assert_eq!(credential.source, API_KEY_ENV);
    }

    #[test]
    fn a_missing_store_reports_signed_out_not_expired() {
        let error = resolve_from(None, &[PathBuf::from("/nonexistent/providers.json")])
            .expect_err("no credential");
        assert!(error.to_string().contains("not signed in"));
    }

    #[test]
    fn an_expired_store_resolves_to_a_flagged_credential() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("providers.json");
        std::fs::write(&path, store(&entry("cline", "lapsed", 1))).expect("write store");

        let credential = resolve_from(None, &[path]).expect("a lapsed credential resolves");
        assert!(credential.expired);
    }
}
