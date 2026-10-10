//! Strict, read-only reader for the official Devin CLI credential file.
//!
//! Only the top-level windsurf_api_key and optional api_server_url are
//! consumed. Credential parse errors intentionally omit parser diagnostics
//! and all input values.

use std::fmt;
use std::io::Read;
use std::path::Path;

use crate::error::{AppError, Result};

pub const API_ORIGIN: &str = "https://server.codeium.com";
const MAX_CREDENTIAL_FILE_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct Credentials {
    pub(super) api_key: String,
    pub fingerprint: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("api_key", &"[REDACTED]")
            .field("fingerprint", &"[REDACTED]")
            .finish()
    }
}

#[derive(serde::Deserialize)]
struct CredentialsFile {
    #[serde(default)]
    windsurf_api_key: Option<String>,
    #[serde(default)]
    api_server_url: Option<String>,
}

pub fn read_from(path: &Path) -> Result<Credentials> {
    let file = std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AppError::Credentials(format!(
                "Devin CLI credentials not found at {}; sign in with Devin CLI and retry",
                crate::display::sanitize_untrusted_path(path)
            ))
        } else {
            AppError::io_at(path, error)
        }
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_CREDENTIAL_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io_at(path, error))?;
    if bytes.len() > MAX_CREDENTIAL_FILE_BYTES {
        return Err(malformed());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| malformed())?;
    let parsed: CredentialsFile = toml::from_str(text).map_err(|_| malformed())?;
    if let Some(server) = parsed.api_server_url.as_deref()
        && server.trim_end_matches('/') != API_ORIGIN
    {
        return Err(AppError::Credentials(
            "Devin CLI credentials specify an unsupported API destination; use the official CLI login and retry".into(),
        ));
    }
    let api_key = parsed
        .windsurf_api_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(strip_bearer_prefix)
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_graphic()))
        .ok_or_else(malformed)?;
    let fingerprint = crate::cache::fingerprint_of(&api_key);
    Ok(Credentials {
        api_key,
        fingerprint,
    })
}

fn strip_bearer_prefix(value: String) -> String {
    if value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
    {
        value[7..].trim().to_string()
    } else {
        value
    }
}

fn malformed() -> AppError {
    AppError::Credentials(
        "Devin CLI credentials are missing or malformed; sign in with the official Devin CLI and retry".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(td: &TempDir, contents: &str) -> std::path::PathBuf {
        let path = td.path().join("credentials.toml");
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn reads_only_the_existing_cli_key_and_scopes_its_cache_identity() {
        let td = TempDir::new().unwrap();
        let path = write(
            &td,
            r#"windsurf_api_key = "canary-secret-value"
api_server_url = "https://server.codeium.com/"
"#,
        );
        let credentials = read_from(&path).unwrap();
        assert_eq!(credentials.api_key, "canary-secret-value");
        assert_eq!(
            credentials.fingerprint,
            crate::cache::fingerprint_of("canary-secret-value")
        );
        assert_eq!(credentials.fingerprint.len(), 16);
        assert_ne!(credentials.fingerprint, credentials.api_key);
    }

    #[test]
    fn accepts_an_optional_bearer_prefix_without_retaining_it() {
        let td = TempDir::new().unwrap();
        let path = write(&td, r#"windsurf_api_key = "Bearer key-123""#);
        assert_eq!(read_from(&path).unwrap().api_key, "key-123");
    }

    #[test]
    fn never_echoes_credential_input_in_errors_or_debug_output() {
        let td = TempDir::new().unwrap();
        let path = write(
            &td,
            r#"windsurf_api_key = "canary-secret-value"
api_server_url = "https://attacker.invalid"
"#,
        );
        let error = read_from(&path).unwrap_err().to_string();
        assert!(!error.contains("canary-secret-value"));
        assert!(!error.contains("attacker.invalid"));

        let path = write(&td, r#"windsurf_api_key = "canary-secret-value""#);
        let credentials = read_from(&path).unwrap();
        let debug = format!("{credentials:?}");
        assert!(!debug.contains("canary-secret-value"));
        assert!(!debug.contains(&credentials.fingerprint));
    }

    #[test]
    fn malformed_or_non_header_safe_tokens_are_rejected_without_echo() {
        let td = TempDir::new().unwrap();
        for contents in [
            r#"windsurf_api_key = """#,
            r#"windsurf_api_key = "line break""#,
            "not a toml document",
        ] {
            let path = write(&td, contents);
            let error = read_from(&path).unwrap_err();
            assert!(matches!(error, AppError::Credentials(_)));
            assert!(!error.to_string().contains("line break"));
        }
    }
}
