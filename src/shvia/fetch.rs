//! ShvIA fetch. The API key is passed as `Authorization: Bearer <KEY>`
//! (WITH the `Bearer ` prefix — unlike the Z.AI vendor, which omits it).
//!
//! Mirrors `zai::fetch`: shared cache + flock primitives, fresh-cache
//! short-circuit, and the shared [`crate::outcome`] fallback policy. The cache
//! holds the gateway's own response body, so `parse_cache` is the same
//! projection the live path uses.

use std::time::Duration;

use crate::cache::{Cache, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::usage::ShviaSnapshot;

use super::types::Envelope;

pub const DEFAULT_BASE_URL: &str = "https://ia.blue3.com.br";
const USAGE_PATH: &str = "/api/v1/usage";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub usage: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self::from_base_url(DEFAULT_BASE_URL)
    }
}

impl Endpoints {
    /// Build the endpoint set from a gateway base URL, joining the fixed
    /// `/api/v1/usage` path. A trailing slash on `base_url` is tolerated.
    pub fn from_base_url(base_url: &str) -> Self {
        let trimmed = base_url.trim_end_matches('/');
        Self {
            usage: format!("{trimmed}{USAGE_PATH}"),
        }
    }
}

/// This vendor's [`Outcome`](crate::outcome::Outcome) — the shared shape,
/// specialised to its snapshot.
pub type FetchOutcome = crate::outcome::Outcome<ShviaSnapshot>;

pub async fn fetch_snapshot(
    client: &reqwest::Client,
    api_key: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
    config_plan: Option<&str>,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;

    if let Some(bytes) = cache.fresh_payload(cache_ttl)?
        && let Ok(outcome) = reuse(bytes, cache, false, config_plan)
    {
        return Ok(outcome);
    }
    // Corrupt fresh cache: fall through to a live fetch rather than serve a
    // snapshot with three empty windows, which reads as "nothing used".

    match fetch_live(client, &endpoints.usage, api_key).await {
        Ok((bytes, env)) => {
            // Only a validated envelope reaches the cache.
            cache.write_payload(&bytes)?;
            Ok(crate::outcome::Outcome::fresh(
                env.into_snapshot(config_plan),
            ))
        }
        Err(e) if e.is_transient() => fallback_silent(cache, config_plan, e),
        Err(AppError::Http { status, body }) => {
            cache.mark_stale();
            let last_error = Some(cache.write_last_error(status, &body));
            fallback_with_error(
                cache,
                last_error,
                config_plan,
                AppError::Http { status, body },
            )
        }
        Err(e) => {
            cache.mark_stale();
            let last_error = Some(cache.write_last_error(0, &e.to_string()));
            fallback_with_error(cache, last_error, config_plan, e)
        }
    }
}

fn reuse(bytes: Vec<u8>, cache: &Cache, stale: bool, plan: Option<&str>) -> Result<FetchOutcome> {
    Ok(crate::outcome::Outcome::cached(
        parse_cache(&bytes, plan)?,
        cache,
        stale,
    ))
}

fn parse_cache(bytes: &[u8], plan: Option<&str>) -> Result<ShviaSnapshot> {
    let env: Envelope = serde_json::from_slice(bytes)?;
    Ok(env.into_snapshot(plan))
}

fn fallback_silent(cache: &Cache, plan: Option<&str>, original: AppError) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, None, original, |bytes| parse_cache(bytes, plan))
}

fn fallback_with_error(
    cache: &Cache,
    last_error: Option<(u16, String)>,
    plan: Option<&str>,
    original: AppError,
) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, last_error, original, |bytes| {
        parse_cache(bytes, plan)
    })
}

/// Returns the raw bytes (for the cache) alongside the *validated* envelope,
/// so the caller cannot accidentally cache a body it never checked.
async fn fetch_live(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
) -> Result<(Vec<u8>, Envelope)> {
    let resp = tokio::time::timeout(
        HTTP_TIMEOUT,
        client
            .get(url)
            .header("Authorization", format!("Bearer {api_key}")) // WITH `Bearer ` prefix.
            .header("Accept", "application/json")
            .send(),
    )
    .await
    .map_err(|_| AppError::Transport(format!("shvia timeout: {url}")))??;

    let status = resp.status();
    let bytes = crate::vendor::read_body_capped(resp, crate::vendor::MAX_BODY_BYTES).await?;

    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes).chars().take(200).collect();
        return Err(AppError::Http {
            status: status.as_u16(),
            body,
        });
    }

    // Sanity check we got a valid envelope. Schema drift surfaces here.
    let env: Envelope = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Schema(format!("shvia usage response: {e}")))?;
    Ok((bytes, env))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cache_fixture() -> (TempDir, Cache) {
        let td = TempDir::new().unwrap();
        let cache = Cache::at(td.path().join("shvia"));
        cache.ensure_dir().unwrap();
        (td, cache)
    }

    #[tokio::test]
    async fn live_200_parses_all_three_windows() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/usage")
            .match_header("authorization", "Bearer fake-key")
            .with_status(200)
            .with_body(
                r#"{
                    "today": {"used": 1234, "limit": 100000, "remaining": 98766, "reset_at": "2026-06-24T00:00:00Z"},
                    "week":  {"used": 250000, "limit": 500000, "remaining": 250000, "reset_at": "2026-06-29T00:00:00Z"},
                    "month": {"used": 40000, "limit": -1, "remaining": null, "reset_at": "2026-07-01T00:00:00Z"}
                }"#,
            )
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = Endpoints::from_base_url(&server.url());
        let out = fetch_snapshot(
            &client,
            "fake-key",
            &cache,
            &endpoints,
            Duration::from_secs(0),
            None,
        )
        .await
        .unwrap();

        assert_eq!(out.snapshot.plan, "ShvIA");
        assert_eq!(out.snapshot.today.as_ref().unwrap().used, 1234);
        // week 250000/500000 = 50%.
        assert_eq!(out.snapshot.week.as_ref().unwrap().utilization_pct(), 50);
        // month is unlimited (limit -1).
        assert!(out.snapshot.month.as_ref().unwrap().is_unlimited());
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn http_401_falls_back_to_cache_when_present() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/usage")
            .with_status(401)
            .with_body(r#"{"error":"unauthorized"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let seed = r#"{
            "week": {"used": 100, "limit": 1000, "remaining": 900, "reset_at": "2026-06-29T00:00:00Z"}
        }"#;
        cache.write_payload(seed.as_bytes()).unwrap();

        let client = reqwest::Client::new();
        let endpoints = Endpoints::from_base_url(&server.url());
        let out = fetch_snapshot(
            &client,
            "k",
            &cache,
            &endpoints,
            Duration::from_secs(0),
            None,
        )
        .await
        .unwrap();

        assert!(out.stale);
        assert_eq!(out.snapshot.week.as_ref().unwrap().used, 100);
        assert_eq!(out.snapshot.week.as_ref().unwrap().utilization_pct(), 10);
        assert_eq!(out.last_error.as_ref().map(|(c, _)| *c), Some(401));
    }

    /// A cold cache has nothing to show, so the rejected key has to be the
    /// error — the shared policy in `outcome::fallback`, asserted here because
    /// this vendor used to return "shvia: no usable cache" instead.
    #[tokio::test]
    async fn http_401_with_no_cache_reports_the_rejection() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/usage")
            .with_status(401)
            .with_body(r#"{"error":"unauthorized"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = Endpoints::from_base_url(&server.url());
        let err = fetch_snapshot(
            &client,
            "k",
            &cache,
            &endpoints,
            Duration::from_secs(0),
            None,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, AppError::Http { status: 401, .. }), "{err:?}");
    }

    #[test]
    fn endpoints_from_base_url_trims_trailing_slash() {
        let e = Endpoints::from_base_url("https://ia.blue3.com.br/");
        assert_eq!(e.usage, "https://ia.blue3.com.br/api/v1/usage");
    }
}
