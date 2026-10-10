//! Read Devin quota status and keep only normalized metrics in the local cache.
//!
//! The CLI key travels only as the `metadata.apiKey` field of the JSON request
//! body, to the fixed Cognition host. Neither the raw response nor the
//! credential is persisted.

use std::io::ErrorKind;
use std::sync::LazyLock;
use std::time::Duration;

use crate::cache::{Cache, acquire_lock_async, atomic_write};
use crate::error::{AppError, Result};
use crate::usage::{DevinSnapshot, UsageWindow};
use crate::vendor::{MAX_BODY_BYTES, read_body_capped};

use super::creds::{API_ORIGIN, Credentials};
use super::types::{DAILY_WINDOW, WEEKLY_WINDOW, parse_response};

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
// Devin-owned login marker: only the 16-hex truncated SHA-256 fingerprint,
// stored in plain text like the payload's account field, never the CLI key.
const ACCOUNT_FILE: &str = ".account";
const USER_STATUS_PATH: &str = "/exa.seat_management_pb.SeatManagementService/GetUserStatus";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub base: String,
}

impl Endpoints {
    pub fn user_status_url(&self) -> String {
        format!("{}{}", self.base.trim_end_matches('/'), USER_STATUS_PATH)
    }
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            base: API_ORIGIN.to_string(),
        }
    }
}

pub type FetchOutcome = crate::outcome::Outcome<DevinSnapshot>;

/// The client refuses every redirect, so the CLI key is never forwarded or
/// re-sent through one, including a same-origin redirect.
fn build_client(timeout: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// Devin keeps its own client instead of the process-wide one that the widget
/// and TUI share: that client follows redirects, and this provider must never
/// send the CLI key through one.
static CLIENT: LazyLock<Option<reqwest::Client>> =
    LazyLock::new(|| build_client(HTTP_TIMEOUT).ok());

fn http_client() -> Result<&'static reqwest::Client> {
    CLIENT
        .as_ref()
        .ok_or_else(|| AppError::Other("Devin HTTP client initialization failed".into()))
}

/// Production entry point; resolves the official CLI file on every call,
/// including cache hits, so a changed login cannot inherit another account's
/// cached quota. Credentials are resolved before any client is built, so a
/// missing login costs nothing.
pub async fn fetch_snapshot(
    cfg: &crate::config::DevinConfig,
    cache: &Cache,
    cache_ttl: Duration,
) -> Result<FetchOutcome> {
    let credentials = super::resolve_credentials(cfg)?;
    fetch_snapshot_with(
        http_client()?,
        &credentials,
        &super::client_version(),
        cache,
        &Endpoints::default(),
        cache_ttl,
    )
    .await
}

async fn fetch_snapshot_with(
    client: &reqwest::Client,
    credentials: &Credentials,
    client_version: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;

    bind_cache_login(cache, &credentials.fingerprint)?;

    if let Some(bytes) = cache.fresh_payload(cache_ttl)?
        && let Ok(outcome) = reuse_cache(&bytes, cache, false, &credentials.fingerprint)
    {
        return Ok(outcome);
    }

    match usage_call(client, credentials, client_version, endpoints).await {
        Ok(snapshot) => {
            cache.write_payload(&serde_json::to_vec(&snapshot_to_cache(
                &snapshot,
                &credentials.fingerprint,
            ))?)?;
            Ok(crate::outcome::Outcome::fresh(snapshot))
        }
        Err(error) if error.is_transient() => {
            fallback_silent(cache, &credentials.fingerprint, error)
        }
        Err(error) => {
            cache.mark_stale();
            let (code, message) = error_to_pair(&error);
            let diagnostic = cache.write_last_error(code, &message);
            fallback_with_error(cache, diagnostic, &credentials.fingerprint, error)
        }
    }
}

// Called under the fetch lock. Cache::forget/clear_backoff do not own .account;
// it may survive either operation and is reconciled here before cache reuse.
fn bind_cache_login(cache: &Cache, fingerprint: &str) -> Result<()> {
    let account_path = cache.dir().join(ACCOUNT_FILE);
    let account = match std::fs::read(&account_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(AppError::io_at(&account_path, error)),
    };
    let payload = cache.maybe_payload()?;
    let valid_payload = payload
        .as_deref()
        .is_some_and(|bytes| parse_cache_at(bytes, fingerprint).is_ok());
    let same_login = match account.as_deref() {
        Some(account) => account == fingerprint.as_bytes(),
        None => valid_payload,
    };

    // The credential behind this cache changed hands or was never bound: the
    // sidecars describe a previous or unestablished login and must not shape
    // the new one's refresh; a valid current-login payload is retained.
    if !same_login
        || payload
            .as_deref()
            .is_some_and(|bytes| belongs_to_other_login(bytes, fingerprint))
    {
        // Strict cleanup intentionally stays Devin-local until a separate
        // shared-cache change can centralize ownership and the sidecar list
        // with Cache::forget.
        for path in [
            cache.payload_path(),
            cache.stale_path(),
            cache.last_error_path(),
            cache.retry_after_path(),
        ] {
            if valid_payload && path == cache.payload_path() {
                continue;
            }
            if let Err(error) = std::fs::remove_file(&path)
                && error.kind() != ErrorKind::NotFound
            {
                return Err(AppError::io_at(path, error));
            }
        }
    }
    if account.as_deref() != Some(fingerprint.as_bytes()) {
        atomic_write(&account_path, fingerprint.as_bytes())?;
    }
    Ok(())
}

async fn usage_call(
    client: &reqwest::Client,
    credentials: &Credentials,
    client_version: &str,
    endpoints: &Endpoints,
) -> Result<DevinSnapshot> {
    let body = serde_json::json!({
        "metadata": {
            "apiKey": &credentials.api_key,
            "ideName": "devin",
            "ideVersion": client_version,
            "extensionVersion": client_version,
            "locale": "en"
        }
    });
    // The client's own timeout bounds the request; the error is rebuilt from
    // fixed text so nothing from the request (URL, headers) can reach a log.
    let response = client
        .post(endpoints.user_status_url())
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("connect-protocol-version", "1")
        .json(&body)
        .send()
        .await
        .map_err(|error| {
            AppError::Transport(if error.is_timeout() {
                "Devin status request timed out".into()
            } else {
                "Devin status request failed".into()
            })
        })?;

    let status = response.status();
    let bytes = read_body_capped(response, MAX_BODY_BYTES).await?;
    if !status.is_success() {
        // The response body is never copied into the error: a 401/403 from a
        // key-bearing endpoint may echo account detail. The real status is
        // kept, and `Cache::write_last_error` redacts 401/403 messages.
        return Err(AppError::Http {
            status: status.as_u16(),
            body: format!("Devin status endpoint returned HTTP {}", status.as_u16()),
        });
    }
    parse_response(&bytes)
}

/// Only an HTTP status or a schema message survives into `.last_error` and the
/// tooltip. Everything else (I/O while reading the body, client setup) is
/// collapsed to fixed text on purpose: those errors can carry a path or a
/// request detail, and this is a key-bearing request. Model Studio exposes
/// `e.to_string()` here; the loss of diagnosis is accepted for Devin.
fn error_to_pair(error: &AppError) -> (u16, String) {
    match error {
        AppError::Http { status, body } => (*status, body.clone()),
        AppError::Schema(message) => (0, message.clone()),
        _ => (0, "Devin status request failed; details suppressed".into()),
    }
}

/// True only for a payload that names a *different* login. A payload that
/// does not parse or carries no identity is left for the normal replace-on-
/// success path rather than treated as another account's.
fn belongs_to_other_login(bytes: &[u8], fingerprint: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| {
            value
                .get("account")
                .and_then(serde_json::Value::as_str)
                .map(|account| account != fingerprint)
        })
        .unwrap_or(false)
}

fn fallback_silent(cache: &Cache, fingerprint: &str, original: AppError) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, None, original, |bytes| {
        parse_cache_at(bytes, fingerprint)
    })
}

fn fallback_with_error(
    cache: &Cache,
    last_error: (u16, String),
    fingerprint: &str,
    original: AppError,
) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, Some(last_error), original, |bytes| {
        parse_cache_at(bytes, fingerprint)
    })
}

fn reuse_cache(
    bytes: &[u8],
    cache: &Cache,
    stale: bool,
    fingerprint: &str,
) -> Result<FetchOutcome> {
    let snapshot = parse_cache_at(bytes, fingerprint)?;
    Ok(crate::outcome::Outcome::cached(snapshot, cache, stale))
}

fn snapshot_to_cache(snapshot: &DevinSnapshot, fingerprint: &str) -> serde_json::Value {
    let window = |window: &Option<UsageWindow>| match window {
        Some(window) => serde_json::json!({
            "used_pct": window.utilization_pct,
            "reset_unix": window.resets_at.map(|at| at.timestamp()),
        }),
        None => serde_json::Value::Null,
    };
    serde_json::json!({
        "account": fingerprint,
        "daily": window(&snapshot.daily),
        "weekly": window(&snapshot.weekly),
        "overage_balance_micros": snapshot.overage_balance_micros,
    })
}

fn parse_cache_at(bytes: &[u8], fingerprint: &str) -> Result<DevinSnapshot> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| AppError::Schema("Devin cache was invalid JSON".into()))?;
    if value.get("account").and_then(serde_json::Value::as_str) != Some(fingerprint) {
        return Err(AppError::Schema(
            "Devin cache belongs to a different login; refetching".into(),
        ));
    }
    let invalid = |field: &str| AppError::Schema(format!("Devin cache: invalid {field}"));
    let window = |field: &str, duration| -> Result<Option<UsageWindow>> {
        let Some(value) = value.get(field) else {
            return Err(invalid(field));
        };
        if value.is_null() {
            return Ok(None);
        }
        let pct = value
            .get("used_pct")
            .and_then(serde_json::Value::as_i64)
            .filter(|pct| (0..=100).contains(pct))
            .ok_or_else(|| invalid(field))? as i32;
        let reset = match value.get("reset_unix") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => {
                let seconds = value
                    .as_i64()
                    .filter(|seconds| *seconds >= 0)
                    .ok_or_else(|| invalid(field))?;
                Some(chrono::DateTime::from_timestamp(seconds, 0).ok_or_else(|| invalid(field))?)
            }
        };
        Ok(Some(UsageWindow {
            utilization_pct: pct,
            resets_at: reset,
            window_duration: duration,
        }))
    };
    let overage_balance_micros = match value.get("overage_balance_micros") {
        Some(serde_json::Value::Null) => None,
        Some(value) => Some(value.as_i64().ok_or_else(|| invalid("overage balance"))?),
        None => return Err(invalid("overage balance")),
    };
    Ok(DevinSnapshot {
        daily: window("daily", DAILY_WINDOW)?,
        weekly: window("weekly", WEEKLY_WINDOW)?,
        overage_balance_micros,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const CANARY: &str = "DevinSecretCanary-593827";
    /// Distinct from the fallback, so the body matchers prove the caller's
    /// version reaches the request rather than a constant.
    const TEST_CLIENT_VERSION: &str = "3000.11.3";

    fn credentials(secret: &str) -> (TempDir, Credentials) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("credentials.toml");
        std::fs::write(&path, format!("windsurf_api_key = {secret:?}\n")).unwrap();
        let credentials = super::super::creds::read_from(&path).unwrap();
        (dir, credentials)
    }

    fn cache_fixture() -> (TempDir, Cache) {
        let dir = TempDir::new().unwrap();
        let cache = Cache::at(dir.path().join("devin"));
        cache.ensure_dir().unwrap();
        (dir, cache)
    }

    /// The production client itself, so the redirect tests exercise the real
    /// no-redirect policy rather than a test-local copy of it.
    fn safe_client() -> reqwest::Client {
        http_client().unwrap().clone()
    }

    fn client_with_timeout(timeout: Duration) -> reqwest::Client {
        build_client(timeout).unwrap()
    }

    fn response_body() -> String {
        serde_json::json!({
            "userStatus": {"planStatus": {
                "dailyQuotaRemainingPercent": 100,
                "dailyQuotaResetAtUnix": "1791014400",
                "weeklyQuotaRemainingPercent": "69",
                "weeklyQuotaResetAtUnix": 1791100800,
                "overageBalanceMicros": "9168615"
            }}
        })
        .to_string()
    }

    fn request_mock(server: &mut mockito::ServerGuard, secret: &str) -> mockito::Mock {
        server
            .mock("POST", USER_STATUS_PATH)
            .match_header("content-type", "application/json")
            .match_header("connect-protocol-version", "1")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "metadata": {
                    "apiKey": secret,
                    "ideName": "devin",
                    "ideVersion": TEST_CLIENT_VERSION,
                    "extensionVersion": TEST_CLIENT_VERSION,
                    "locale": "en"
                }
            })))
    }

    fn snapshot() -> DevinSnapshot {
        DevinSnapshot {
            daily: Some(UsageWindow {
                utilization_pct: 42,
                resets_at: chrono::DateTime::from_timestamp(1_791_014_400, 0),
                window_duration: DAILY_WINDOW,
            }),
            weekly: None,
            overage_balance_micros: Some(9_168_615),
        }
    }

    #[tokio::test]
    async fn request_uses_the_cli_protocol_and_cache_contains_only_normalized_values() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let mut server = mockito::Server::new_async().await;
        let request = request_mock(&mut server, CANARY)
            .with_status(200)
            .with_body(response_body())
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        let outcome = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints { base: server.url() },
            Duration::ZERO,
        )
        .await
        .unwrap();
        request.assert_async().await;
        assert_eq!(outcome.snapshot.daily.as_ref().unwrap().utilization_pct, 0);
        assert_eq!(
            outcome.snapshot.weekly.as_ref().unwrap().utilization_pct,
            31
        );
        assert_eq!(outcome.snapshot.overage_balance_micros, Some(9_168_615));
        let stored = std::fs::read_to_string(cache.payload_path()).unwrap();
        assert!(!stored.contains(CANARY));
        assert!(!stored.contains("userStatus"));
        assert!(!stored.contains("dailyQuotaRemainingPercent"));
        assert_eq!(
            parse_cache_at(stored.as_bytes(), &credentials.fingerprint).unwrap(),
            outcome.snapshot
        );
    }

    #[tokio::test]
    async fn authentication_errors_hide_response_bodies_from_cache_and_fallback() {
        for status in [401, 403] {
            let (_credential_dir, credentials) = credentials(CANARY);
            let mut server = mockito::Server::new_async().await;
            let request = server
                .mock("POST", USER_STATUS_PATH)
                .with_status(status)
                .with_body(format!("credential echo: {CANARY}"))
                .create_async()
                .await;
            let (_cache_dir, cache) = cache_fixture();
            cache
                .write_payload(
                    &serde_json::to_vec(&snapshot_to_cache(&snapshot(), &credentials.fingerprint))
                        .unwrap(),
                )
                .unwrap();

            let outcome = fetch_snapshot_with(
                &safe_client(),
                &credentials,
                TEST_CLIENT_VERSION,
                &cache,
                &Endpoints { base: server.url() },
                Duration::ZERO,
            )
            .await
            .unwrap();
            request.assert_async().await;
            assert!(outcome.stale);
            let (code, error) = outcome.last_error.unwrap();
            assert_eq!(code, status as u16);
            assert_eq!(error, crate::error::AUTH_FAILURE_MESSAGE);
            assert!(!error.contains(CANARY));
            for item in std::fs::read_dir(cache.dir()).unwrap() {
                let item = item.unwrap();
                if item.file_type().unwrap().is_file() {
                    let bytes = std::fs::read(item.path()).unwrap();
                    assert!(!String::from_utf8_lossy(&bytes).contains(CANARY));
                }
            }
        }
    }

    #[tokio::test]
    async fn missing_cli_credentials_fail_before_creating_or_touching_the_cache() {
        let dir = TempDir::new().unwrap();
        let credential_path = dir.path().join("missing-credentials.toml");
        let cache_dir = dir.path().join("cache");
        let cache = Cache::at(cache_dir.clone());
        let cfg = crate::config::DevinConfig {
            credentials_path: Some(credential_path),
            ..Default::default()
        };

        let error = fetch_snapshot(&cfg, &cache, Duration::ZERO)
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::Credentials(_)));
        assert!(!cache_dir.exists());
    }

    #[tokio::test]
    async fn a_timed_out_response_body_is_a_transient_transport_error() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let mut server = mockito::Server::new_async().await;
        let request = request_mock(&mut server, CANARY)
            .with_status(200)
            .with_chunked_body(|writer| {
                writer.write_all(b"{")?;
                std::thread::sleep(Duration::from_millis(500));
                writer.write_all(b"}")
            })
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();

        let error = fetch_snapshot_with(
            &client_with_timeout(Duration::from_millis(200)),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints { base: server.url() },
            Duration::ZERO,
        )
        .await
        .unwrap_err();

        request.assert_async().await;
        assert!(matches!(error, AppError::Transport(_)), "{error:?}");
        assert!(!cache.payload_path().exists());
    }

    #[tokio::test]
    async fn a_cross_origin_redirect_is_not_followed_with_the_cli_key() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let mut server = mockito::Server::new_async().await;
        let request = request_mock(&mut server, CANARY)
            .with_status(307)
            .with_header("location", "https://attacker.invalid/collect")
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        let error = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints { base: server.url() },
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        request.assert_async().await;
        assert!(matches!(error, AppError::Http { status: 307, .. }));
        assert!(!error.to_string().contains(CANARY));
    }

    #[tokio::test]
    async fn a_same_origin_redirect_is_not_followed_with_the_cli_key() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let mut server = mockito::Server::new_async().await;
        let destination = format!("{}/redirected", server.url());
        let request = request_mock(&mut server, CANARY)
            .with_status(307)
            .with_header("location", &destination)
            .create_async()
            .await;
        let redirected = server
            .mock("POST", "/redirected")
            .expect(0)
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        let error = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints { base: server.url() },
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        request.assert_async().await;
        redirected.assert_async().await;
        assert!(matches!(error, AppError::Http { status: 307, .. }));
        assert!(!error.to_string().contains(CANARY));
    }

    #[tokio::test]
    async fn a_cache_for_another_login_is_refetched_not_reused() {
        let (_old_dir, old_credentials) = credentials("old-account-key");
        let (_new_dir, credentials) = credentials("new-account-key");
        let mut server = mockito::Server::new_async().await;
        let request = request_mock(&mut server, "new-account-key")
            .with_status(200)
            .with_body(response_body())
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        let mut old_snapshot = snapshot();
        old_snapshot.daily.as_mut().unwrap().utilization_pct = 99;
        cache
            .write_payload(
                &serde_json::to_vec(&snapshot_to_cache(
                    &old_snapshot,
                    &old_credentials.fingerprint,
                ))
                .unwrap(),
            )
            .unwrap();
        let outcome = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints { base: server.url() },
            Duration::from_secs(3600),
        )
        .await
        .unwrap();
        request.assert_async().await;
        assert_eq!(outcome.snapshot.daily.unwrap().utilization_pct, 0);
    }

    #[tokio::test]
    async fn an_auth_rejection_without_a_cache_keeps_the_real_status() {
        for status in [401, 403] {
            let (_credential_dir, credentials) = credentials(CANARY);
            let mut server = mockito::Server::new_async().await;
            let request = server
                .mock("POST", USER_STATUS_PATH)
                .with_status(status)
                .with_body(format!("credential echo: {CANARY}"))
                .create_async()
                .await;
            let (_cache_dir, cache) = cache_fixture();

            let error = fetch_snapshot_with(
                &safe_client(),
                &credentials,
                TEST_CLIENT_VERSION,
                &cache,
                &Endpoints { base: server.url() },
                Duration::ZERO,
            )
            .await
            .unwrap_err();

            request.assert_async().await;
            assert!(
                matches!(&error, AppError::Http { status: got, .. } if *got == status as u16),
                "{error:?}"
            );
            assert!(!error.to_string().contains(CANARY));
            assert!(
                error
                    .user_message()
                    .contains(crate::error::AUTH_FAILURE_MESSAGE)
            );
            assert_eq!(
                cache.read_last_error(),
                Some((
                    status as u16,
                    crate::error::AUTH_FAILURE_MESSAGE.to_string()
                ))
            );
        }
    }

    #[tokio::test]
    async fn a_changed_login_forgets_the_previous_logins_payload_and_backoff() {
        let (_old_dir, old_credentials) = credentials("old-account-key");
        let (_new_dir, credentials) = credentials("new-account-key");
        let mut server = mockito::Server::new_async().await;
        let request = request_mock(&mut server, "new-account-key")
            .with_status(200)
            .with_body(response_body())
            .expect(1)
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(
                &serde_json::to_vec(&snapshot_to_cache(
                    &snapshot(),
                    &old_credentials.fingerprint,
                ))
                .unwrap(),
            )
            .unwrap();
        cache.mark_stale();
        cache.write_last_error(500, "previous login failed");
        cache.note_rate_limit_at(std::time::SystemTime::now());
        assert!(cache.backoff_remaining().is_some());

        let outcome = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints { base: server.url() },
            Duration::from_secs(3600),
        )
        .await
        .unwrap();

        request.assert_async().await;
        assert!(!outcome.stale);
        assert!(outcome.last_error.is_none());
        assert_eq!(outcome.snapshot.daily.unwrap().utilization_pct, 0);
        assert!(!cache.is_stale());
        assert!(cache.read_last_error().is_none());
        assert!(cache.backoff_remaining().is_none());
        let stored = std::fs::read(cache.payload_path()).unwrap();
        assert!(parse_cache_at(&stored, &credentials.fingerprint).is_ok());
    }

    #[tokio::test]
    async fn a_cold_cache_backoff_is_scoped_to_the_login_across_reopens() {
        let (_old_dir, old_credentials) = credentials(CANARY);
        let (_new_dir, new_credentials) = credentials("new-account-key");
        let mut server = mockito::Server::new_async().await;
        let old_request = request_mock(&mut server, CANARY)
            .with_status(429)
            .expect(1)
            .create_async()
            .await;
        let new_request = request_mock(&mut server, "new-account-key")
            .with_status(200)
            .with_body(response_body())
            .expect(1)
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        let endpoints = Endpoints { base: server.url() };
        let client = safe_client();

        let first = fetch_snapshot_with(
            &client,
            &old_credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &endpoints,
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(matches!(first, AppError::Http { status: 429, .. }));
        assert!(!cache.payload_path().exists());
        assert!(cache.backoff_remaining().is_some());
        let retry_after = std::fs::read(cache.retry_after_path()).unwrap();

        let cache = Cache::at(cache.dir().to_path_buf());
        let same_login = fetch_snapshot_with(
            &client,
            &old_credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &endpoints,
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(matches!(same_login, AppError::Http { status: 429, .. }));
        assert_eq!(
            std::fs::read(cache.retry_after_path()).unwrap(),
            retry_after
        );
        old_request.assert_async().await;

        let cache = Cache::at(cache.dir().to_path_buf());
        let changed_login = fetch_snapshot_with(
            &client,
            &new_credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &endpoints,
            Duration::from_secs(3600),
        )
        .await
        .unwrap();
        new_request.assert_async().await;
        assert!(!changed_login.stale);
        assert!(changed_login.last_error.is_none());
        assert!(!cache.is_stale());
        assert!(cache.read_last_error().is_none());
        assert!(cache.backoff_remaining().is_none());
        let stored = std::fs::read(cache.payload_path()).unwrap();
        assert_eq!(
            parse_cache_at(&stored, &new_credentials.fingerprint).unwrap(),
            changed_login.snapshot
        );
        let account = std::fs::read_to_string(cache.dir().join(ACCOUNT_FILE)).unwrap();
        assert_eq!(account, new_credentials.fingerprint);
        assert_eq!(account.len(), 16);
        assert!(
            account
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert!(!account.contains(CANARY));
        assert!(!account.contains("new-account-key"));
    }

    #[test]
    fn unowned_backoff_is_discarded_once_when_migrating_a_cold_cache() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let (_cache_dir, cache) = cache_fixture();
        cache.mark_stale();
        cache.write_last_error(429, "legacy rate limit");
        assert!(!cache.payload_path().exists());
        assert!(!cache.dir().join(ACCOUNT_FILE).exists());
        assert!(cache.backoff_remaining().is_some());

        bind_cache_login(&cache, &credentials.fingerprint).unwrap();
        assert!(!cache.is_stale());
        assert!(cache.read_last_error().is_none());
        assert!(cache.backoff_remaining().is_none());
        assert_eq!(
            std::fs::read_to_string(cache.dir().join(ACCOUNT_FILE)).unwrap(),
            credentials.fingerprint
        );

        cache.write_last_error(429, "current login rate limit");
        let retry_after = std::fs::read(cache.retry_after_path()).unwrap();
        let cache = Cache::at(cache.dir().to_path_buf());
        bind_cache_login(&cache, &credentials.fingerprint).unwrap();
        assert_eq!(
            std::fs::read(cache.retry_after_path()).unwrap(),
            retry_after
        );
        assert_eq!(
            cache.read_last_error(),
            Some((429, "current login rate limit".into()))
        );
    }

    #[test]
    fn failed_cache_invalidation_does_not_record_a_new_login() {
        let (_cache_dir, cache) = cache_fixture();
        let old_fingerprint = crate::cache::fingerprint_of("old-account-key");
        let new_fingerprint = crate::cache::fingerprint_of("new-account-key");
        let account_path = cache.dir().join(ACCOUNT_FILE);
        std::fs::write(&account_path, &old_fingerprint).unwrap();
        cache
            .write_payload(
                &serde_json::to_vec(&snapshot_to_cache(&snapshot(), &old_fingerprint)).unwrap(),
            )
            .unwrap();
        cache.write_last_error(429, "old login rate limit");
        // remove_file rejects a directory on Unix and Windows. The payload is
        // removed first, so this exercises partial cleanup without committing
        // the new identity.
        std::fs::create_dir(cache.stale_path()).unwrap();

        let error = bind_cache_login(&cache, &new_fingerprint).unwrap_err();
        assert!(matches!(error, AppError::Io { .. }));
        assert!(!cache.payload_path().exists());
        assert!(cache.stale_path().is_dir());
        assert!(cache.last_error_path().exists());
        assert!(cache.retry_after_path().exists());
        assert_eq!(
            std::fs::read_to_string(account_path).unwrap(),
            old_fingerprint
        );
    }

    #[test]
    fn a_valid_current_payload_survives_an_old_login_marker() {
        let (_cache_dir, cache) = cache_fixture();
        let old_fingerprint = crate::cache::fingerprint_of("old-account-key");
        let fingerprint = crate::cache::fingerprint_of("new-account-key");
        let payload = serde_json::to_vec(&snapshot_to_cache(&snapshot(), &fingerprint)).unwrap();
        cache.write_payload(&payload).unwrap();
        std::fs::write(cache.dir().join(ACCOUNT_FILE), old_fingerprint).unwrap();
        cache.mark_stale();
        cache.write_last_error(429, "old login rate limit");

        bind_cache_login(&cache, &fingerprint).unwrap();

        assert_eq!(cache.maybe_payload().unwrap(), Some(payload));
        assert!(!cache.stale_path().exists());
        assert!(!cache.last_error_path().exists());
        assert!(!cache.retry_after_path().exists());
        assert_eq!(
            std::fs::read_to_string(cache.dir().join(ACCOUNT_FILE)).unwrap(),
            fingerprint
        );
    }

    #[test]
    fn a_surviving_login_marker_after_forget_is_safe_to_rebind() {
        let (_cache_dir, cache) = cache_fixture();
        let fingerprint = crate::cache::fingerprint_of("old-account-key");
        bind_cache_login(&cache, &fingerprint).unwrap();
        cache.write_last_error(429, "old login rate limit");
        cache.forget();
        assert_eq!(
            std::fs::read_to_string(cache.dir().join(ACCOUNT_FILE)).unwrap(),
            fingerprint
        );

        bind_cache_login(&cache, &fingerprint).unwrap();
        assert!(!cache.payload_path().exists());
        assert!(!cache.retry_after_path().exists());

        let new_fingerprint = crate::cache::fingerprint_of("new-account-key");
        bind_cache_login(&cache, &new_fingerprint).unwrap();
        assert_eq!(
            std::fs::read_to_string(cache.dir().join(ACCOUNT_FILE)).unwrap(),
            new_fingerprint
        );
    }

    #[tokio::test]
    async fn the_same_login_keeps_its_backoff() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let mut server = mockito::Server::new_async().await;
        let request = request_mock(&mut server, CANARY)
            .expect(0)
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(
                &serde_json::to_vec(&snapshot_to_cache(&snapshot(), &credentials.fingerprint))
                    .unwrap(),
            )
            .unwrap();
        cache.note_rate_limit_at(std::time::SystemTime::now());

        let outcome = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints { base: server.url() },
            Duration::ZERO,
        )
        .await
        .unwrap();

        request.assert_async().await;
        assert_eq!(outcome.snapshot, snapshot());
        assert!(cache.backoff_remaining().is_some());
        assert_eq!(
            std::fs::read_to_string(cache.dir().join(ACCOUNT_FILE)).unwrap(),
            credentials.fingerprint
        );
    }

    #[tokio::test]
    async fn a_corrupt_cache_is_replaced_by_a_successful_fetch() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let mut server = mockito::Server::new_async().await;
        let request = request_mock(&mut server, CANARY)
            .with_status(200)
            .with_body(response_body())
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        cache.write_payload(b"not-json").unwrap();

        let outcome = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints { base: server.url() },
            Duration::from_secs(3600),
        )
        .await
        .unwrap();

        request.assert_async().await;
        assert!(!outcome.stale);
        assert_eq!(outcome.snapshot.daily.unwrap().utilization_pct, 0);
        let stored = std::fs::read(cache.payload_path()).unwrap();
        assert!(parse_cache_at(&stored, &credentials.fingerprint).is_ok());
    }

    #[tokio::test]
    async fn an_expired_cache_is_refreshed_and_replaced() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let mut server = mockito::Server::new_async().await;
        let request = request_mock(&mut server, CANARY)
            .with_status(200)
            .with_body(response_body())
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(
                &serde_json::to_vec(&snapshot_to_cache(&snapshot(), &credentials.fingerprint))
                    .unwrap(),
            )
            .unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(cache.payload_path())
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
            .unwrap();

        let outcome = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints { base: server.url() },
            Duration::from_secs(3600),
        )
        .await
        .unwrap();

        request.assert_async().await;
        assert!(!outcome.stale);
        assert_eq!(outcome.snapshot.daily.unwrap().utilization_pct, 0);
    }

    #[tokio::test]
    async fn rate_limits_arm_backoff_and_skip_the_next_network_request() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let mut server = mockito::Server::new_async().await;
        let request = request_mock(&mut server, CANARY)
            .with_status(429)
            .with_body("rate-limit-body-must-not-be-persisted")
            .expect(1)
            .create_async()
            .await;
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(
                &serde_json::to_vec(&snapshot_to_cache(&snapshot(), &credentials.fingerprint))
                    .unwrap(),
            )
            .unwrap();
        let endpoints = Endpoints { base: server.url() };

        let first = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &endpoints,
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert!(first.stale);
        assert_eq!(first.last_error.as_ref().map(|(code, _)| *code), Some(429));
        assert!(cache.backoff_remaining().is_some());
        assert!(
            !std::fs::read_to_string(cache.last_error_path())
                .unwrap()
                .contains("rate-limit-body-must-not-be-persisted")
        );

        let second = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &endpoints,
            Duration::ZERO,
        )
        .await
        .unwrap();
        request.assert_async().await;
        assert!(second.stale);
        assert_eq!(second.last_error.as_ref().map(|(code, _)| *code), Some(429));
    }

    #[tokio::test]
    async fn a_fresh_identity_matching_cache_does_not_touch_the_network() {
        let (_credential_dir, credentials) = credentials(CANARY);
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(
                &serde_json::to_vec(&snapshot_to_cache(&snapshot(), &credentials.fingerprint))
                    .unwrap(),
            )
            .unwrap();
        let outcome = fetch_snapshot_with(
            &safe_client(),
            &credentials,
            TEST_CLIENT_VERSION,
            &cache,
            &Endpoints {
                base: "http://127.0.0.1:1".into(),
            },
            Duration::from_secs(60),
        )
        .await
        .unwrap();
        assert_eq!(outcome.snapshot, snapshot());
        assert!(!outcome.stale);
    }

    #[test]
    fn cache_requires_identity_and_valid_metric_shapes() {
        let snapshot = snapshot();
        let encoded = serde_json::to_vec(&snapshot_to_cache(&snapshot, "account-a")).unwrap();
        assert_eq!(parse_cache_at(&encoded, "account-a").unwrap(), snapshot);
        assert!(parse_cache_at(&encoded, "account-b").is_err());
        for invalid in [
            serde_json::json!({"account":"account-a","daily":{"used_pct":101},"weekly":null,"overage_balance_micros":null}),
            serde_json::json!({"account":"account-a","daily":null,"weekly":null}),
            serde_json::json!({"account":"account-a","daily":{"used_pct":1,"reset_unix":-1},"weekly":null,"overage_balance_micros":null}),
        ] {
            assert!(parse_cache_at(invalid.to_string().as_bytes(), "account-a").is_err());
        }
    }
}
