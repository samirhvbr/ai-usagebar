use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cache::{Cache, acquire_lock_async, atomic_write};
use crate::error::{AppError, Result};
use crate::usage::MuseSnapshot;
use crate::vendor::{MAX_BODY_BYTES, read_body_capped};

use super::{creds::Credentials, types::Response};

pub const REQUEST_INTERVAL: Duration = Duration::from_secs(300);
pub type FetchOutcome = crate::outcome::Outcome<MuseSnapshot>;

pub struct Endpoints {
    pub base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            base: "https://api.meta.ai".into(),
        }
    }
}

impl Endpoints {
    fn url(&self) -> String {
        format!("{}/muse-code/key", self.base)
    }
}

#[derive(Serialize, Deserialize)]
struct Cached {
    account: String,
    response: Response,
}

#[derive(Serialize, Deserialize)]
struct RequestRecord {
    at: DateTime<Utc>,
    error: Option<(u16, String)>,
}

pub async fn fetch_snapshot(
    cfg: &crate::config::MuseConfig,
    cache: &Cache,
) -> Result<FetchOutcome> {
    let credentials = super::creds::read_from(&super::creds::path(cfg)?)?;
    let endpoints = Endpoints::default();
    fetch_snapshot_at(&credentials, cache, &endpoints, Utc::now()).await
}

/// The clock, credential and endpoint are injected for hermetic tests.
pub async fn fetch_snapshot_at(
    credentials: &Credentials,
    cache: &Cache,
    endpoints: &Endpoints,
    now: DateTime<Utc>,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), Duration::from_secs(20)).await?;
    let record_path = cache
        .dir()
        .join(format!("request-{}.json", credentials.fingerprint));
    let parse_cache = |bytes: &[u8]| -> Result<MuseSnapshot> {
        let cached: Cached = serde_json::from_slice(bytes)
            .map_err(|_| AppError::Schema("Muse Code quota cache is invalid".into()))?;
        if cached.account != credentials.fingerprint {
            return Err(AppError::Schema(
                "Muse Code quota cache belongs to a different login".into(),
            ));
        }
        cached.response.snapshot(now)
    };
    if record_path.exists() {
        // A damaged ledger must not bypass the request bound. The user can
        // remove this vendor cache explicitly to recover it.
        let bytes = std::fs::read(&record_path).map_err(|e| AppError::io_at(&record_path, e))?;
        let record: RequestRecord = serde_json::from_slice(&bytes).map_err(|_| {
            AppError::Other("Muse Code request record is invalid; clear its vendor cache".into())
        })?;
        if now.signed_duration_since(record.at).num_seconds() < REQUEST_INTERVAL.as_secs() as i64 {
            if record.error.is_none()
                && let Some(bytes) = cache.maybe_payload()?
                && let Ok(snapshot) = parse_cache(&bytes)
            {
                return Ok(crate::outcome::Outcome::cached(snapshot, cache, false));
            }
            let (code, message) = record.error.unwrap_or_else(|| (0,
                "Muse Code quota request was interrupted or its observation expired; next request is deferred for five minutes".into()));
            return crate::outcome::fallback(
                cache,
                Some((code, message.clone())),
                AppError::Other(message),
                parse_cache,
            );
        }
    }
    // This endpoint also supplies a standing inference key. Bound *every*
    // attempt, including failures, crashes and forced refreshes, before sending.
    atomic_write(
        &record_path,
        &serde_json::to_vec(&RequestRecord {
            at: now,
            error: Some((
                0,
                "Muse Code quota request did not complete; retry after five minutes".into(),
            )),
        })?,
    )?;
    let result = request(credentials, endpoints).await.and_then(|response| {
        let snapshot = response.snapshot(now)?;
        Ok((response, snapshot))
    });
    match result {
        Ok((response, snapshot)) => {
            cache.write_payload(&serde_json::to_vec(&Cached {
                account: credentials.fingerprint.clone(),
                response,
            })?)?;
            atomic_write(
                &record_path,
                &serde_json::to_vec(&RequestRecord {
                    at: now,
                    error: None,
                })?,
            )?;
            Ok(crate::outcome::Outcome::fresh(snapshot))
        }
        Err(error) => {
            let code = match &error {
                AppError::Http { status, .. } => *status,
                _ => 0,
            };
            let pair = cache.write_last_error(code, &error.to_string());
            cache.mark_stale();
            atomic_write(
                &record_path,
                &serde_json::to_vec(&RequestRecord {
                    at: now,
                    error: Some(pair.clone()),
                })?,
            )?;
            crate::outcome::fallback(cache, Some(pair), error, parse_cache)
        }
    }
}

async fn request(credentials: &Credentials, endpoints: &Endpoints) -> Result<Response> {
    // Even a same-origin redirect is rejected: no token forwarding, no extra
    // mint call, and the route cannot come from a credential's api_base_url.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()?;
    let response = client
        .post(endpoints.url())
        .bearer_auth(&credentials.token)
        .header("x-api-version", "1.0.0")
        .header("Accept", "application/json")
        .json(&serde_json::json!({"onboard":false}))
        .send()
        .await
        .map_err(|_| AppError::Transport("Muse Code quota request failed".into()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Http {
            status: status.as_u16(),
            body: if matches!(status.as_u16(), 401 | 403) {
                "Muse Code login rejected; run `muse login`".into()
            } else {
                format!("Muse Code quota returned HTTP {}", status.as_u16())
            },
        });
    }
    let bytes = read_body_capped(response, MAX_BODY_BYTES).await?;
    serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Schema("Muse Code quota response schema mismatch".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1791936000, 0).unwrap()
    }
    fn credential(token: &str) -> Credentials {
        Credentials {
            token: token.into(),
            fingerprint: crate::cache::fingerprint_of(token),
        }
    }
    fn body() -> String {
        serde_json::json!({"is_subs_active":true,"subs_tier_name":"Muse Code Power Usage",
        "api_key":"ResponseSecretCanary-9123","user_email":"private@example.test",
        "subs_usage":{"window":{"used_percent":38,"window_duration_mins":300,"resets_at":1791942677},
        "weekly":{"used_percent":34,"resets_at":1792368000}}}).to_string()
    }
    #[tokio::test]
    async fn quota_request_is_bounded_and_cache_contains_no_secrets() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/muse-code/key")
            .match_header("authorization", "Bearer dca:test-login")
            .match_header("x-api-version", "1.0.0")
            .match_body("{\"onboard\":false}")
            .with_body(body())
            .expect(1)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(dir.path().join("cache"));
        let endpoint = Endpoints { base: server.url() };
        let creds = credential("dca:test-login");
        assert!(
            !fetch_snapshot_at(&creds, &cache, &endpoint, now())
                .await
                .unwrap()
                .stale
        );
        assert_eq!(
            fetch_snapshot_at(
                &creds,
                &cache,
                &endpoint,
                now() + chrono::Duration::seconds(1)
            )
            .await
            .unwrap()
            .snapshot
            .weekly
            .unwrap()
            .utilization_pct,
            34
        );
        mock.assert_async().await;
        for path in std::fs::read_dir(cache.dir()).unwrap() {
            let content = std::fs::read(path.unwrap().path()).unwrap();
            let content = String::from_utf8_lossy(&content);
            for secret in ["dca:test-login", "ResponseSecretCanary", "private@example"] {
                assert!(!content.contains(secret));
            }
        }
    }
    #[tokio::test]
    async fn omitted_idle_quota_keeps_previous_observation_stale_and_bounds_failed_reads() {
        let mut server = mockito::Server::new_async().await;
        let first = server
            .mock("POST", "/muse-code/key")
            .with_body(body())
            .expect(1)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(dir.path().join("cache"));
        let endpoint = Endpoints { base: server.url() };
        let creds = credential("dca:test-login");
        fetch_snapshot_at(&creds, &cache, &endpoint, now())
            .await
            .unwrap();
        first.remove_async().await;
        let second = server
            .mock("POST", "/muse-code/key")
            .with_body("{\"is_subs_active\":true}")
            .expect(1)
            .create_async()
            .await;
        let later = now() + chrono::Duration::seconds(300);
        let outcome = fetch_snapshot_at(&creds, &cache, &endpoint, later)
            .await
            .unwrap();
        assert!(outcome.stale);
        assert_eq!(outcome.snapshot.weekly.unwrap().utilization_pct, 34);
        assert!(
            fetch_snapshot_at(
                &creds,
                &cache,
                &endpoint,
                later + chrono::Duration::seconds(1)
            )
            .await
            .unwrap()
            .stale
        );
        second.assert_async().await;
        second.remove_async().await;
        let other = server
            .mock("POST", "/muse-code/key")
            .match_header("authorization", "Bearer dca:other")
            .with_body("{\"is_subs_active\":true}")
            .expect(1)
            .create_async()
            .await;
        assert!(
            fetch_snapshot_at(&credential("dca:other"), &cache, &endpoint, later)
                .await
                .is_err()
        );
        other.assert_async().await;
    }

    #[tokio::test]
    async fn interrupted_request_and_rejected_login_do_not_trigger_immediate_retries() {
        let mut server = mockito::Server::new_async().await;
        let never = server
            .mock("POST", "/muse-code/key")
            .expect(0)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(dir.path().join("interrupted"));
        cache.ensure_dir().unwrap();
        let creds = credential("dca:interrupted");
        let path = cache
            .dir()
            .join(format!("request-{}.json", creds.fingerprint));
        atomic_write(
            &path,
            &serde_json::to_vec(&RequestRecord {
                at: now(),
                error: Some((0, "Previous request interrupted".into())),
            })
            .unwrap(),
        )
        .unwrap();
        let endpoint = Endpoints { base: server.url() };
        assert!(
            fetch_snapshot_at(&creds, &cache, &endpoint, now())
                .await
                .is_err()
        );
        never.assert_async().await;
        never.remove_async().await;
        let denied = server
            .mock("POST", "/muse-code/key")
            .with_status(401)
            .expect(1)
            .create_async()
            .await;
        let cache = Cache::at(dir.path().join("denied"));
        let creds = credential("dca:denied");
        assert!(
            fetch_snapshot_at(&creds, &cache, &endpoint, now())
                .await
                .is_err()
        );
        assert!(
            fetch_snapshot_at(
                &creds,
                &cache,
                &endpoint,
                now() + chrono::Duration::seconds(1)
            )
            .await
            .is_err()
        );
        denied.assert_async().await;
    }
    #[tokio::test]
    async fn redirects_and_echoed_error_bodies_never_forward_or_expose_credentials() {
        let mut server = mockito::Server::new_async().await;
        let redirected = server.mock("POST", "/other").expect(0).create_async().await;
        for status in [302, 401, 500] {
            let mock = server
                .mock("POST", "/muse-code/key")
                .with_status(status)
                .with_header("location", &format!("{}/other", server.url()))
                .with_body("dca:SecretCanary-92143")
                .expect(1)
                .create_async()
                .await;
            let dir = tempfile::tempdir().unwrap();
            let cache = Cache::at(dir.path().join("cache"));
            let endpoint = Endpoints { base: server.url() };
            let result = fetch_snapshot_at(&credential("dca:test"), &cache, &endpoint, now()).await;
            assert!(!result.err().unwrap().to_string().contains("SecretCanary"));
            assert!(
                !std::fs::read_to_string(cache.last_error_path())
                    .unwrap()
                    .contains("SecretCanary")
            );
            mock.assert_async().await;
            mock.remove_async().await;
        }
        redirected.assert_async().await;
    }

    #[tokio::test]
    async fn network_transport_failure_is_transient_on_cold_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(dir.path().join("cache"));
        let endpoint = Endpoints {
            base: "http://127.0.0.1:1".into(),
        };
        let creds = credential("dca:test");
        let err = fetch_snapshot_at(&creds, &cache, &endpoint, now())
            .await
            .unwrap_err();
        assert!(err.is_transient(), "expected transient error, got {err:?}");
    }
}
