//! Read-only Devin CLI quota provider.
//!
//! This provider reuses the official CLI's existing local credential file.
//! It never logs in, refreshes, or writes credentials. The API route is fixed
//! to the host used by the CLI, and its HTTP client rejects all redirects so
//! the token is never forwarded or re-sent through a redirect.

pub mod creds;
pub mod fetch;
pub mod types;
pub mod vendor;

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::DevinConfig;
use crate::error::{AppError, Result};

pub const DATA_DIR_NAME: &str = "devin";
pub const CREDENTIALS_FILE_NAME: &str = "credentials.toml";

/// Sent as the request's `ideVersion` and `extensionVersion` when the CLI's
/// own version record is absent or unreadable. The endpoint accepts any
/// SemVer `N.N.N`.
pub const FALLBACK_CLIENT_VERSION: &str = "0.0.0";
const VERSION_FILE_MAX_BYTES: u64 = 4 * 1024;

/// Which per-user base directory a Devin CLI file lives under.
#[derive(Clone, Copy)]
enum BaseDir {
    Data,
    Cache,
}

/// The Devin CLI follows XDG on every Unix, macOS included: its docs put the
/// credential at `$XDG_DATA_HOME/devin` or `~/.local/share/devin` on macOS and
/// Linux, and at `%APPDATA%\devin` on Windows. So `directories` is consulted
/// only on Windows (Roaming for data, Local for cache); on macOS it would
/// answer `~/Library`, where the CLI writes nothing.
fn platform_dir(kind: BaseDir) -> Result<PathBuf> {
    if cfg!(windows) {
        let base = directories::BaseDirs::new()
            .ok_or_else(|| AppError::Other("could not resolve the Windows known folders".into()))?;
        return Ok(match kind {
            BaseDir::Data => base.data_dir(),
            BaseDir::Cache => base.cache_dir(),
        }
        .to_path_buf());
    }
    let var = match kind {
        BaseDir::Data => "XDG_DATA_HOME",
        BaseDir::Cache => "XDG_CACHE_HOME",
    };
    xdg_dir(kind, std::env::var_os(var), crate::cache::home_dir)
}

/// Pure XDG rule: the variable wins only when absolute, as the XDG Base
/// Directory spec requires; otherwise the documented default under home.
fn xdg_dir(
    kind: BaseDir,
    var: Option<OsString>,
    home: impl FnOnce() -> Result<PathBuf>,
) -> Result<PathBuf> {
    if let Some(dir) = var.map(PathBuf::from).filter(|dir| dir.is_absolute()) {
        return Ok(dir);
    }
    let home = home()?;
    Ok(match kind {
        BaseDir::Data => home.join(".local").join("share"),
        BaseDir::Cache => home.join(".cache"),
    })
}

/// Pure path resolver with the data directory injected. An explicit
/// `credentials_path` is returned as is, without resolving any directory, so
/// the override works on a machine with no resolvable home.
fn credentials_path_with(
    cfg: &DevinConfig,
    data_dir: impl FnOnce() -> Result<PathBuf>,
) -> Result<PathBuf> {
    if let Some(path) = cfg.credentials_path.as_ref() {
        return Ok(path.clone());
    }
    Ok(data_dir()?.join(DATA_DIR_NAME).join(CREDENTIALS_FILE_NAME))
}

pub fn credentials_path(cfg: &DevinConfig) -> Result<PathBuf> {
    credentials_path_with(cfg, || platform_dir(BaseDir::Data))
}

pub fn resolve_credentials(cfg: &DevinConfig) -> Result<creds::Credentials> {
    creds::read_from(&credentials_path(cfg)?)
}

/// The version the CLI records in `devin/cli/cached_version.json` under the
/// cache directory (`%LOCALAPPDATA%` on Windows, `${XDG_CACHE_HOME:-~/.cache}`
/// elsewhere), or [`FALLBACK_CLIENT_VERSION`]. Never an error: the request
/// works with any SemVer, so a missing record only costs accuracy.
pub fn client_version() -> String {
    platform_dir(BaseDir::Cache)
        .map(|dir| client_version_at(&version_file_in(&dir)))
        .unwrap_or_else(|_| FALLBACK_CLIENT_VERSION.to_string())
}

fn version_file_in(cache_dir: &Path) -> PathBuf {
    cache_dir
        .join(DATA_DIR_NAME)
        .join("cli")
        .join("cached_version.json")
}

/// Read `{"latest": "N.N.N"}`, bounded and strict. The file names the newest
/// release the CLI has seen, which is the version it runs after its own
/// auto-update; anything that is not plain `N.N.N` falls back.
fn client_version_at(path: &Path) -> String {
    #[derive(serde::Deserialize)]
    struct VersionRecord {
        latest: String,
    }

    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|file| {
        file.take(VERSION_FILE_MAX_BYTES + 1)
            .read_to_end(&mut bytes)
    });
    if read.is_err() || bytes.len() as u64 > VERSION_FILE_MAX_BYTES {
        return FALLBACK_CLIENT_VERSION.to_string();
    }
    serde_json::from_slice::<VersionRecord>(&bytes)
        .ok()
        .map(|record| record.latest)
        .filter(|version| is_plain_semver(version))
        .unwrap_or_else(|| FALLBACK_CLIENT_VERSION.to_string())
}

fn is_plain_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            (1..=9).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn no_home() -> Result<PathBuf> {
        panic!("an absolute XDG variable must not resolve home")
    }

    #[test]
    fn default_path_is_the_injected_data_dir_plus_devin_credentials() {
        let data_dir = PathBuf::from("/data");
        let path = credentials_path_with(&DevinConfig::default(), || Ok(data_dir.clone())).unwrap();
        assert_eq!(path, data_dir.join("devin").join("credentials.toml"));
    }

    #[test]
    fn xdg_defaults_follow_the_cli_docs_on_every_unix_including_macos() {
        let home = || Ok(PathBuf::from("/home/u"));
        assert_eq!(
            xdg_dir(BaseDir::Data, None, home).unwrap(),
            Path::new("/home/u").join(".local").join("share")
        );
        assert_eq!(
            xdg_dir(BaseDir::Cache, None, home).unwrap(),
            Path::new("/home/u").join(".cache")
        );
    }

    #[test]
    fn an_absolute_xdg_variable_wins_and_a_relative_one_is_ignored() {
        // A literal absolute path, not `std::env::temp_dir()`: the test must
        // not read `TMPDIR`/`TEMP`, and `/xdg` is relative on Windows.
        let absolute = PathBuf::from(if cfg!(windows) { r"C:\xdg" } else { "/xdg" });
        assert_eq!(
            xdg_dir(BaseDir::Data, Some(absolute.clone().into()), no_home).unwrap(),
            absolute
        );
        assert_eq!(
            xdg_dir(BaseDir::Cache, Some("relative/cache".into()), || Ok(
                PathBuf::from("/home/u")
            ))
            .unwrap(),
            Path::new("/home/u").join(".cache")
        );
    }

    #[test]
    fn an_explicit_path_never_resolves_a_data_dir() {
        let cfg = DevinConfig {
            credentials_path: Some("/explicit/credentials.toml".into()),
            ..Default::default()
        };
        let path = credentials_path_with(&cfg, || {
            panic!("an override must not touch the home or data directory")
        })
        .unwrap();
        assert_eq!(path, Path::new("/explicit/credentials.toml"));
    }

    #[test]
    fn an_unresolvable_data_dir_is_an_error_not_a_guess() {
        let error = credentials_path_with(&DevinConfig::default(), || {
            Err(AppError::Other("no data dir".into()))
        })
        .unwrap_err();
        assert!(matches!(error, AppError::Other(_)));
    }

    #[test]
    fn the_client_version_comes_from_the_clis_version_record() {
        let td = TempDir::new().unwrap();
        let path = version_file_in(td.path());
        assert_eq!(
            path,
            td.path()
                .join("devin")
                .join("cli")
                .join("cached_version.json")
        );
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"latest":"3000.11.3"}"#).unwrap();
        assert_eq!(client_version_at(&path), "3000.11.3");
    }

    #[test]
    fn an_absent_or_unexpected_version_record_falls_back() {
        let td = TempDir::new().unwrap();
        let path = td.path().join("cached_version.json");
        assert_eq!(client_version_at(&path), FALLBACK_CLIENT_VERSION);
        for contents in [
            "",
            "not json",
            r#"{"current":"1.2.3"}"#,
            r#"{"latest":3000}"#,
            r#"{"latest":"3000.11"}"#,
            r#"{"latest":"3000.11.3-beta"}"#,
            r#"{"latest":"v3000.11.3"}"#,
            r#"{"latest":"1.2.3\nX-Injected: 1"}"#,
            r#"{"latest":"1234567890.0.0"}"#,
        ] {
            std::fs::write(&path, contents).unwrap();
            assert_eq!(
                client_version_at(&path),
                FALLBACK_CLIENT_VERSION,
                "{contents:?}"
            );
        }
        std::fs::write(&path, " ".repeat(5 * 1024)).unwrap();
        assert_eq!(client_version_at(&path), FALLBACK_CLIENT_VERSION);
    }
}
