//! Read the Cursor IDE's own session token out of its local `state.vscdb`.
//!
//! Cursor has no documented usage API or API key for personal quota — every
//! community tool that shows it (cursor-stats, cursor-usage-tracker, etc.)
//! reads the same place: a SQLite key-value store the Cursor IDE itself
//! maintains at `.../User/globalStorage/state.vscdb` (the same `state.vscdb`
//! every VS Code-family app uses for `ItemTable`-shaped extension/global
//! state), under the key `cursorAuth/accessToken`. That value is a JWT whose
//! `sub` claim (`auth0|<userId>`) is combined with the raw token into the
//! `WorkosCursorSessionToken` cookie the dashboard's own usage call expects —
//! see `fetch.rs`.

use std::path::{Path, PathBuf};
use std::{hash::Hash, hash::Hasher};

use rusqlite::{Connection, OpenFlags};

use crate::display::sanitize_untrusted_path;
use crate::error::{AppError, Result};

const TOKEN_KEY: &str = "cursorAuth/accessToken";

/// Default location of Cursor's local state database. This is Cursor's own
/// per-OS convention (same one every VS Code-family app uses for its user
/// data), not ai-usagebar's XDG cache — conveniently identical to what
/// `directories::BaseDirs::config_dir()` already resolves on every platform:
///   - Linux: `~/.config`
///   - macOS: `~/Library/Application Support`
///   - Windows: `%APPDATA%` (Roaming)
pub fn default_db_path() -> Result<PathBuf> {
    let base = directories::BaseDirs::new().ok_or_else(|| {
        AppError::Other("could not resolve the platform config directory (no HOME?)".into())
    })?;
    Ok(base
        .config_dir()
        .join("Cursor")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb"))
}

/// Read the raw `cursorAuth/accessToken` value from `path`. A missing file or
/// missing row means "never signed in to Cursor" — reported as a credentials
/// error (like a missing `~/.claude/.credentials.json`) rather than a network
/// or schema failure, so the widget's `⚠` tooltip tells the user to sign in
/// rather than implying the API is down.
pub fn read_access_token(path: &Path) -> Result<String> {
    if !path.exists() {
        return Err(AppError::Credentials(format!(
            "Cursor database not found at {}. Open the Cursor IDE and sign in at least once, \
             then try again.",
            sanitize_untrusted_path(path)
        )));
    }
    // Read-only: this file is Cursor's own live state, not ours to lock for
    // writing. SQLite allows concurrent readers, so this is safe alongside a
    // running Cursor IDE.
    let conn =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| {
            AppError::Credentials(format!(
                "could not open Cursor database at {}: {e}",
                sanitize_untrusted_path(path)
            ))
        })?;
    let token: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            [TOKEN_KEY],
            |row| row.get(0),
        )
        .map_err(|_| {
            AppError::Credentials(format!(
                "no Cursor session found in {}. Sign in to the Cursor IDE, then try again.",
                sanitize_untrusted_path(path)
            ))
        })?;
    if token.trim().is_empty() {
        return Err(AppError::Credentials(
            "Cursor session token is empty. Sign in to the Cursor IDE again.".into(),
        ));
    }
    Ok(token)
}

/// Default location of the `cursor-agent` CLI's own login state — a plain
/// JSON file, not the IDE's `state.vscdb`. Written by the headless
/// `cursor-agent` tool, so it stays
/// populated on machines that never run the desktop IDE at all.
pub fn default_agent_auth_path() -> Result<PathBuf> {
    let base = directories::BaseDirs::new().ok_or_else(|| {
        AppError::Other("could not resolve the platform config directory (no HOME?)".into())
    })?;
    Ok(base.config_dir().join("cursor").join("auth.json"))
}

/// Read `cursor-agent`'s `accessToken` out of its `auth.json`. Same error
/// shape as [`read_access_token`] (missing file / missing field / empty
/// value are all a [`AppError::Credentials`]) so callers can treat both
/// sources interchangeably.
pub fn read_agent_access_token(path: &Path) -> Result<String> {
    if !path.exists() {
        return Err(AppError::Credentials(format!(
            "cursor-agent auth file not found at {}. Run `cursor-agent` and sign in at least \
             once, then try again.",
            sanitize_untrusted_path(path)
        )));
    }
    let bytes = std::fs::read(path).map_err(|e| AppError::io_at(path, e))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
        AppError::Credentials(format!(
            "could not parse {}: {e}",
            sanitize_untrusted_path(path)
        ))
    })?;
    let token = value
        .get("accessToken")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            AppError::Credentials(format!(
                "no accessToken in {}. Sign in with `cursor-agent` again.",
                sanitize_untrusted_path(path)
            ))
        })?;
    Ok(token.to_string())
}

/// Resolve a Cursor session token from either source. The IDE's `state.vscdb`
/// is tried first (it is the live, continuously-refreshed source when the
/// desktop app is actually running); a text-only / headless machine that has
/// never opened the IDE falls back to whatever `cursor-agent` last wrote to
/// its own `auth.json`. If the agent file exists but cannot be used, its error
/// is surfaced so a headless user gets an actionable diagnostic. The IDE's
/// error remains the one surfaced when both sources are absent, since it names
/// the more commonly expected path.
pub fn resolve_access_token(db_path: &Path, agent_auth_path: &Path) -> Result<String> {
    match read_access_token(db_path) {
        Ok(token) => Ok(token),
        Err(_) if !db_path.exists() && agent_auth_path.exists() => {
            read_agent_access_token(agent_auth_path)
        }
        Err(ide_err) => Err(ide_err),
    }
}

/// The two values the `/api/usage` call needs, both derived from the same JWT:
/// the bare user id (a query param) and the `WorkosCursorSessionToken` cookie
/// value (`userId%3A%3Atoken` — literal, pre-encoded `::`, matching what the
/// Cursor dashboard's own JS sends).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAuth {
    pub user_id: String,
    /// Stable, non-plaintext cache identity for the signed-in Cursor account.
    /// The hash is only a change detector: a different toolchain may produce a
    /// different value and force one harmless refetch.
    pub account_key: String,
    pub cookie_value: String,
}

/// Derive [`SessionAuth`] from the raw access token. Fails when the token
/// isn't a decodable JWT or its `sub` claim doesn't have the `issuer|userId`
/// shape every Cursor account token carries — either way the token is
/// unusable, so this is a credentials error, not a schema error (the *shape*
/// of the wire endpoint isn't in play yet at this point).
pub fn session_auth(token: &str) -> Result<SessionAuth> {
    let claims = crate::jwt::claims(token).ok_or_else(|| {
        AppError::Credentials(
            "Cursor session token could not be decoded. Sign in to the Cursor IDE again.".into(),
        )
    })?;
    let sub = claims
        .get("sub")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| AppError::Credentials("Cursor session token has no `sub` claim.".into()))?;
    let user_id = sub
        .split('|')
        .nth(1)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::Credentials(format!(
                "Cursor session token `sub` claim has an unexpected shape: {sub:?}"
            ))
        })?
        .to_string();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    user_id.hash(&mut hasher);
    let account_key = format!("{:016x}", hasher.finish());
    let cookie_value = format!("{user_id}%3A%3A{token}");
    Ok(SessionAuth {
        user_id,
        account_key,
        cookie_value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use tempfile::TempDir;

    /// Build a fake JWT with the given claims (no signature verification,
    /// matching `openai::creds`'s test helper).
    fn fake_jwt(claims: serde_json::Value) -> String {
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
        format!("{header}.{payload}.sig")
    }

    fn seed_db(path: &Path, token: Option<&str>) {
        let conn = Connection::open(path).unwrap();
        conn.execute("CREATE TABLE ItemTable (key TEXT, value TEXT)", [])
            .unwrap();
        if let Some(t) = token {
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                rusqlite::params![TOKEN_KEY, t],
            )
            .unwrap();
        }
    }

    #[test]
    fn default_db_path_ends_with_the_cursor_state_file() {
        let p = default_db_path().unwrap();
        assert!(
            p.ends_with(
                std::path::Path::new("Cursor")
                    .join("User")
                    .join("globalStorage")
                    .join("state.vscdb")
            )
        );
    }

    #[test]
    fn missing_file_is_a_credentials_error_naming_the_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.vscdb");
        let err = read_access_token(&path).unwrap_err();
        match err {
            AppError::Credentials(m) => assert!(m.contains(&path.display().to_string())),
            other => panic!("expected Credentials error, got {other:?}"),
        }
    }

    #[test]
    fn reads_the_token_back_out_of_the_item_table() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.vscdb");
        seed_db(&path, Some("fake-token-value"));
        assert_eq!(read_access_token(&path).unwrap(), "fake-token-value");
    }

    #[test]
    fn missing_row_is_a_credentials_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.vscdb");
        seed_db(&path, None);
        let err = read_access_token(&path).unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)));
    }

    #[test]
    fn empty_token_is_a_credentials_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.vscdb");
        seed_db(&path, Some(""));
        let err = read_access_token(&path).unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)));
    }

    #[test]
    fn session_auth_extracts_user_id_and_builds_the_cookie_value() {
        let token = fake_jwt(serde_json::json!({"sub": "auth0|user_abc123"}));
        let auth = session_auth(&token).unwrap();
        assert_eq!(auth.user_id, "user_abc123");
        assert_eq!(auth.account_key.len(), 16);
        assert!(!auth.account_key.contains("user_abc123"));
        assert_eq!(auth.cookie_value, format!("user_abc123%3A%3A{token}"));
    }

    #[test]
    fn session_auth_account_key_is_stable_and_account_specific() {
        let one = session_auth(&fake_jwt(serde_json::json!({"sub": "auth0|one"}))).unwrap();
        let one_again = session_auth(&fake_jwt(serde_json::json!({"sub": "auth0|one"}))).unwrap();
        let two = session_auth(&fake_jwt(serde_json::json!({"sub": "auth0|two"}))).unwrap();
        assert_eq!(one.account_key, one_again.account_key);
        assert_ne!(one.account_key, two.account_key);
    }

    #[test]
    fn session_auth_rejects_a_non_jwt_token() {
        let err = session_auth("not-a-jwt").unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)));
    }

    #[test]
    fn session_auth_rejects_missing_sub_claim() {
        let token = fake_jwt(serde_json::json!({"other": "value"}));
        let err = session_auth(&token).unwrap_err();
        match err {
            AppError::Credentials(m) => assert!(m.contains("sub")),
            other => panic!("expected Credentials error, got {other:?}"),
        }
    }

    #[test]
    fn session_auth_rejects_sub_without_a_pipe_separated_user_id() {
        let token = fake_jwt(serde_json::json!({"sub": "no-pipe-here"}));
        let err = session_auth(&token).unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)));
    }

    #[test]
    fn default_agent_auth_path_ends_with_cursor_auth_json() {
        let p = default_agent_auth_path().unwrap();
        assert!(p.ends_with(std::path::Path::new("cursor").join("auth.json")));
    }

    #[test]
    fn agent_auth_missing_file_is_a_credentials_error_naming_the_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("auth.json");
        let err = read_agent_access_token(&path).unwrap_err();
        match err {
            AppError::Credentials(m) => assert!(m.contains(&path.display().to_string())),
            other => panic!("expected Credentials error, got {other:?}"),
        }
    }

    #[test]
    fn agent_auth_reads_access_token_out_of_the_json_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(
            &path,
            serde_json::json!({"accessToken": "agent-token-value", "refreshToken": "r"})
                .to_string(),
        )
        .unwrap();
        assert_eq!(read_agent_access_token(&path).unwrap(), "agent-token-value");
    }

    #[test]
    fn agent_auth_missing_field_is_a_credentials_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(&path, serde_json::json!({"refreshToken": "r"}).to_string()).unwrap();
        let err = read_agent_access_token(&path).unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)));
    }

    #[test]
    fn agent_auth_empty_token_is_a_credentials_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(&path, serde_json::json!({"accessToken": ""}).to_string()).unwrap();
        let err = read_agent_access_token(&path).unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)));
    }

    #[test]
    fn agent_auth_malformed_json_is_a_credentials_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(&path, "not json").unwrap();
        let err = read_agent_access_token(&path).unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)));
    }

    #[test]
    fn resolve_prefers_the_ide_db_when_both_are_present() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("state.vscdb");
        seed_db(&db_path, Some("ide-token"));
        let agent_path = dir.path().join("auth.json");
        std::fs::write(
            &agent_path,
            serde_json::json!({"accessToken": "agent-token"}).to_string(),
        )
        .unwrap();
        assert_eq!(
            resolve_access_token(&db_path, &agent_path).unwrap(),
            "ide-token"
        );
    }

    #[test]
    fn resolve_falls_back_to_the_agent_file_when_the_ide_db_is_missing() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("state.vscdb");
        let agent_path = dir.path().join("auth.json");
        std::fs::write(
            &agent_path,
            serde_json::json!({"accessToken": "agent-token"}).to_string(),
        )
        .unwrap();
        assert_eq!(
            resolve_access_token(&db_path, &agent_path).unwrap(),
            "agent-token"
        );
    }

    #[test]
    fn resolve_does_not_hide_an_existing_broken_ide_db_with_the_agent_file() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("state.vscdb");
        seed_db(&db_path, None);
        let agent_path = dir.path().join("auth.json");
        std::fs::write(
            &agent_path,
            serde_json::json!({"accessToken": "agent-token"}).to_string(),
        )
        .unwrap();

        let err = resolve_access_token(&db_path, &agent_path).unwrap_err();
        match err {
            AppError::Credentials(m) => {
                assert!(m.contains(&db_path.display().to_string()));
                assert!(!m.contains(&agent_path.display().to_string()));
            }
            other => panic!("expected Credentials error, got {other:?}"),
        }
    }

    #[test]
    fn resolve_surfaces_the_ide_error_when_both_sources_are_missing() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("state.vscdb");
        let agent_path = dir.path().join("auth.json");
        let err = resolve_access_token(&db_path, &agent_path).unwrap_err();
        match err {
            AppError::Credentials(m) => assert!(m.contains(&db_path.display().to_string())),
            other => panic!("expected Credentials error, got {other:?}"),
        }
    }

    #[test]
    fn resolve_surfaces_the_agent_error_when_its_file_exists_but_is_malformed() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("state.vscdb");
        let agent_path = dir.path().join("auth.json");
        std::fs::write(&agent_path, "not json").unwrap();

        let err = resolve_access_token(&db_path, &agent_path).unwrap_err();
        match err {
            AppError::Credentials(m) => {
                assert!(m.contains(&agent_path.display().to_string()));
                assert!(m.contains("could not parse"));
            }
            other => panic!("expected Credentials error, got {other:?}"),
        }
    }
}
