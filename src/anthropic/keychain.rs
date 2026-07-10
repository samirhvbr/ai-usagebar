//! macOS Keychain access for Claude Code OAuth credentials.
//!
//! On Linux the Claude CLI writes its OAuth state to
//! `~/.claude/.credentials.json`. On macOS, recent Claude Code builds instead
//! store the *same* `{ "claudeAiOauth": …, "mcpOAuth": … }` JSON as a generic
//! password item in the login Keychain (service `Claude Code-credentials`), so
//! the file never exists and a naive read fails with an I/O error.
//!
//! We shell out to the built-in `security(1)` tool rather than pulling in a
//! macOS-only crate (`security-framework`) — it keeps the dependency tree and
//! the Linux build untouched, and mirrors the project's "read what the CLI
//! already wrote" philosophy.

use std::process::Command;

use crate::error::{AppError, Result};

/// Generic-password *service* name Claude Code uses for the credentials blob.
const SERVICE: &str = "Claude Code-credentials";

/// The Keychain item's *account* is the macOS short username. We match on it
/// when updating so we touch exactly the item Claude Code created.
fn account() -> String {
    std::env::var("USER").unwrap_or_default()
}

/// Read the raw credentials JSON from the login Keychain.
///
/// Returns `Ok(None)` when no such item exists (so callers can fall through to
/// the file path / a "run `claude`" error), and `Err` only on an unexpected
/// `security` failure.
pub fn read_raw() -> Result<Option<String>> {
    // Try the account-scoped lookup first (the item Claude Code created for this
    // user), then fall back to a service-only lookup. The item's account is not
    // guaranteed to equal $USER across Claude Code builds/logins, and there is
    // normally exactly one `Claude Code-credentials` item — without this fallback
    // a mismatched account silently looks like "no credentials in Keychain" and
    // the widget surfaces a bogus "file missing" error.
    let acct = account();
    if !acct.is_empty()
        && let Some(v) =
            security_read(&["find-generic-password", "-s", SERVICE, "-a", &acct, "-w"])?
    {
        return Ok(Some(v));
    }
    security_read(&["find-generic-password", "-s", SERVICE, "-w"])
}

/// Run `security` with `args`; `Ok(Some(value))` on success with a non-empty
/// password, `Ok(None)` when the item is absent (non-zero exit — `security`
/// returns 44 / errSecItemNotFound). Never a hard error, so callers can still
/// fall back to the file path / a friendlier "run `claude`" message.
fn security_read(args: &[&str]) -> Result<Option<String>> {
    let out = Command::new("/usr/bin/security")
        .args(args)
        .output()
        .map_err(|e| AppError::Other(format!("could not run `security`: {e}")))?;

    if !out.status.success() {
        return Ok(None);
    }

    let value = String::from_utf8(out.stdout)
        .map_err(|e| AppError::Other(format!("Keychain value was not UTF-8: {e}")))?;
    let value = value.trim_end_matches('\n').to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

/// Persist updated credentials JSON back to the *same* Keychain item, so the
/// widget and Claude Code keep sharing a single source of truth (mirroring how
/// they share one file on Linux). `-U` updates the item in place if it exists.
///
/// Note: the JSON is passed as a `security` argument and is therefore briefly
/// visible in this process's argv (e.g. to `ps`) on the user's own machine.
/// `security` offers no stdin path for the password, and this runs only on the
/// rare proactive token refresh, so we accept the local-only exposure of a
/// secret that already lives in this user's Keychain.
pub fn write_raw(json: &str) -> Result<()> {
    let status = Command::new("/usr/bin/security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            SERVICE,
            "-a",
            &account(),
            "-w",
            json,
        ])
        .status()
        .map_err(|e| AppError::Other(format!("could not run `security`: {e}")))?;

    if status.success() {
        Ok(())
    } else {
        Err(AppError::Other(
            "failed to update Claude credentials in the macOS Keychain".into(),
        ))
    }
}
