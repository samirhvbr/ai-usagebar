use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{AppError, Result};

const LOGIN_HINT: &str = "Muse Code requires an existing file-based OAuth login and issued key; run `muse login` or set [muse-code] credentials_path";
const MAX_CREDENTIAL_BYTES: u64 = 1024 * 1024;

// Deliberately no Debug/Serialize: this record holds a live secret.
pub struct Credentials {
    pub(crate) token: String,
    pub(crate) fingerprint: String,
}

pub fn default_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("MUSE_AUTH_PATH").filter(|value| !value.is_empty()) {
        return Ok(path.into());
    }
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(base.join("muse/auth.json"));
    }
    Ok(crate::cache::home_dir()?.join(".config/muse/auth.json"))
}

pub fn path(cfg: &crate::config::MuseConfig) -> Result<PathBuf> {
    cfg.credentials_path.clone().map_or_else(default_path, Ok)
}

pub fn read_from(path: &Path) -> Result<Credentials> {
    let file = std::fs::File::open(path).map_err(|_| AppError::Credentials(LOGIN_HINT.into()))?;
    let mut bytes = Vec::new();
    file.take(MAX_CREDENTIAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AppError::Credentials(LOGIN_HINT.into()))?;
    if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
        return Err(AppError::Credentials(
            "Muse Code login file exceeds the size limit".into(),
        ));
    }
    parse(&bytes)
}

fn parse(bytes: &[u8]) -> Result<Credentials> {
    #[derive(Deserialize)]
    struct Auth {
        providers: Providers,
    }
    #[derive(Deserialize)]
    struct Providers {
        meta: Meta,
    }
    #[derive(Deserialize)]
    struct Meta {
        mechanism: String,
        access_token: String,
        api_key: String,
    }
    let auth: Auth =
        serde_json::from_slice(bytes).map_err(|_| AppError::Credentials(LOGIN_HINT.into()))?;
    let meta = auth.providers.meta;
    // A standing key must already exist: this reader never performs initial
    // onboarding. An inference key is not an OAuth subscription credential.
    if meta.mechanism != "oauth"
        || !meta.access_token.starts_with("dca:")
        || meta.access_token.len() <= 4
        || meta.api_key.trim().is_empty()
    {
        return Err(AppError::Credentials(LOGIN_HINT.into()));
    }
    Ok(Credentials {
        fingerprint: crate::cache::fingerprint_of(&meta.access_token),
        token: meta.access_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_existing_oauth_login_and_never_quotes_rejected_secrets() {
        let secret = "SecretCanary-48291";
        let body = |mechanism, token, key| {
            serde_json::json!({"providers":{"meta":{
            "mechanism":mechanism,"access_token":token,"api_key":key}}})
            .to_string()
        };
        assert!(parse(body("oauth", "dca:test-login", "standing-key").as_bytes()).is_ok());
        for bytes in [
            body("api_key", secret, secret),
            body("oauth", secret, secret),
            body("oauth", "dca:test-login", ""),
            secret.into(),
        ] {
            let error = parse(bytes.as_bytes()).err().unwrap().to_string();
            assert!(!error.contains(secret));
        }
    }
    #[test]
    fn reads_only_injected_file_without_modifying_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        let bytes = br#"{"providers":{"meta":{"mechanism":"oauth","access_token":"dca:test","api_key":"standing"}}}"#;
        std::fs::write(&path, bytes).unwrap();
        assert!(read_from(&path).is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
