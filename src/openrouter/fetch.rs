//! OpenRouter fetch — combines `/api/v1/credits` and `/api/v1/key` under
//! the shared cache + flock primitives.

use std::time::Duration;

use crate::cache::{Cache, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::usage::OpenRouterSnapshot;

use super::types::{CreditsData, KeyData, OrEnvelope, combine};

pub const BASE_URL: &str = "https://openrouter.ai/api/v1";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub credits: String,
    pub key: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            credits: format!("{BASE_URL}/credits"),
            key: format!("{BASE_URL}/key"),
        }
    }
}

/// This vendor's [`Outcome`](crate::outcome::Outcome) — the shared shape,
/// specialised to its snapshot.
pub type FetchOutcome = crate::outcome::Outcome<OpenRouterSnapshot>;

/// Cache-aware fetch. Mirrors `anthropic::fetch::fetch_snapshot` semantics:
/// fresh cache short-circuits; on failure, fall back to cache + mark stale.
pub async fn fetch_snapshot(
    client: &reqwest::Client,
    api_key: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;

    if let Some(bytes) = cache.fresh_payload(cache_ttl)?
        && let Ok(outcome) = reuse_cache(bytes, cache, false)
    {
        return Ok(outcome);
    }
    // Corrupt fresh cache: fall through to live fetch rather than return a
    // fabricated zero-credit snapshot.

    match fetch_live(client, endpoints, api_key).await {
        Ok((credits, key)) => {
            let snap = combine(credits, key);
            // Serialize back to JSON for the cache.
            let cache_repr = serde_json::json!({
                "snapshot": serde_repr(&snap),
            });
            let bytes = serde_json::to_vec(&cache_repr)?;
            cache.write_payload(&bytes)?;
            Ok(crate::outcome::Outcome::fresh(snap))
        }
        Err(e) if e.is_transient() => fallback_silent(cache, e),
        Err(AppError::Http { status, body }) => {
            cache.mark_stale();
            let last_error = Some(cache.write_last_error(status, &body));
            fallback_with_error(cache, last_error, AppError::Http { status, body })
        }
        Err(e) => {
            cache.mark_stale();
            let last_error = Some(cache.write_last_error(0, &e.to_string()));
            fallback_with_error(cache, last_error, e)
        }
    }
}

fn fallback_silent(cache: &Cache, original: AppError) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, None, original, parse_cache)
}

fn fallback_with_error(
    cache: &Cache,
    last_error: Option<(u16, String)>,
    original: AppError,
) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, last_error, original, parse_cache)
}

fn reuse_cache(bytes: Vec<u8>, cache: &Cache, stale: bool) -> Result<FetchOutcome> {
    let snap = parse_cache(&bytes)?;
    Ok(crate::outcome::Outcome::cached(snap, cache, stale))
}

/// Cached money is required, not optional: a truncated or half-written payload
/// must be refetched rather than rendered as $0.00 with a free-tier badge.
/// `limit`/`limit_remaining` stay optional — the API itself returns them null.
fn parse_cache(bytes: &[u8]) -> Result<OpenRouterSnapshot> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    let s = v
        .get("snapshot")
        .ok_or_else(|| AppError::Schema("openrouter cache missing 'snapshot' field".into()))?;
    let money = |name: &str| -> Result<f64> {
        let n = s
            .get(name)
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| AppError::Schema(format!("openrouter cache missing '{name}'")))?;
        if n.is_finite() && n >= 0.0 {
            Ok(n)
        } else {
            Err(AppError::Schema(format!(
                "openrouter cache '{name}' is not finite and non-negative"
            )))
        }
    };
    let optional_money = |name: &str, nonnegative: bool| -> Result<Option<f64>> {
        match s.get(name) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(value) => {
                let number = value.as_f64().ok_or_else(|| {
                    AppError::Schema(format!("openrouter cache '{name}' is not numeric or null"))
                })?;
                if number.is_finite() && (!nonnegative || number >= 0.0) {
                    Ok(Some(number))
                } else {
                    Err(AppError::Schema(format!(
                        "openrouter cache '{name}' is outside its valid range"
                    )))
                }
            }
        }
    };
    Ok(OpenRouterSnapshot {
        label: s
            .get("label")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AppError::Schema("openrouter cache missing 'label'".into()))?
            .to_string(),
        total_credits: money("total_credits")?,
        total_usage: money("total_usage")?,
        usage_daily: money("usage_daily")?,
        usage_weekly: money("usage_weekly")?,
        usage_monthly: money("usage_monthly")?,
        is_free_tier: s["is_free_tier"]
            .as_bool()
            .ok_or_else(|| AppError::Schema("openrouter cache missing 'is_free_tier'".into()))?,
        limit: optional_money("limit", true)?,
        limit_remaining: optional_money("limit_remaining", false)?,
    })
}

fn serde_repr(snap: &OpenRouterSnapshot) -> serde_json::Value {
    serde_json::json!({
        "label": snap.label,
        "total_credits": snap.total_credits,
        "total_usage": snap.total_usage,
        "usage_daily": snap.usage_daily,
        "usage_weekly": snap.usage_weekly,
        "usage_monthly": snap.usage_monthly,
        "is_free_tier": snap.is_free_tier,
        "limit": snap.limit,
        "limit_remaining": snap.limit_remaining,
    })
}

async fn fetch_live(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    api_key: &str,
) -> Result<(CreditsData, KeyData)> {
    // Fetch in parallel.
    let credits_fut = fetch_one::<CreditsData>(client, &endpoints.credits, api_key);
    let key_fut = fetch_one::<KeyData>(client, &endpoints.key, api_key);
    let (credits, key) = tokio::join!(credits_fut, key_fut);
    Ok((credits?, key?))
}

async fn fetch_one<T: for<'de> serde::Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
) -> Result<T> {
    let resp = tokio::time::timeout(
        HTTP_TIMEOUT,
        client
            .get(url)
            .header("Authorization", format!("Bearer {api_key}"))
            .send(),
    )
    .await
    .map_err(|_| AppError::Transport(format!("openrouter timeout: {url}")))??;

    let status = resp.status();
    let bytes = crate::vendor::read_body_capped(resp, crate::vendor::MAX_BODY_BYTES).await?;

    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes).chars().take(200).collect();
        return Err(AppError::Http {
            status: status.as_u16(),
            body,
        });
    }
    let env: OrEnvelope<T> = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Schema(format!("openrouter {url}: {e}")))?;
    Ok(env.data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cache_fixture() -> (TempDir, Cache) {
        let td = TempDir::new().unwrap();
        let cache = Cache::at(td.path().join("openrouter"));
        cache.ensure_dir().unwrap();
        (td, cache)
    }

    #[tokio::test]
    async fn live_fetch_combines_both_endpoints() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/credits")
            .with_status(200)
            .with_body(r#"{"data":{"total_credits":100.0,"total_usage":25.5}}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/key")
            .with_status(200)
            .with_body(
                r#"{"data":{"label":"prod","limit":50.0,"limit_remaining":24.5,
                "usage":25.5,"usage_daily":1.0,"usage_weekly":7.0,"usage_monthly":25.5,
                "is_free_tier":false}}"#,
            )
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = Endpoints {
            credits: format!("{}/api/v1/credits", server.url()),
            key: format!("{}/api/v1/key", server.url()),
        };
        let out = fetch_snapshot(
            &client,
            "sk-or-test",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert_eq!(out.snapshot.total_credits, 100.0);
        assert_eq!(out.snapshot.total_usage, 25.5);
        assert!((out.snapshot.balance() - 74.5).abs() < 1e-9);
        assert_eq!(out.snapshot.label, "OpenRouter — prod");
        assert!(!out.stale);
    }

    /// With a cache to fall back on, the status rides along as `last_error`
    /// and the user still sees a figure. With a *cold* cache there is no
    /// figure, and the error is all the user gets — so it has to be the real
    /// one. This returned `AppError::Other("openrouter: no usable cache")`
    /// once, which reads as an internal problem on a first run where the
    /// actual cause is a key that was never accepted.
    #[tokio::test]
    async fn an_http_error_with_no_cache_surfaces_the_status_not_a_cache_message() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/credits")
            .with_status(401)
            .with_body(r#"{"error":"unauthorized"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let endpoints = Endpoints {
            credits: format!("{}/api/v1/credits", server.url()),
            key: format!("{}/api/v1/key", server.url()),
        };
        let err = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-or-test",
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(err, AppError::Http { status: 401, .. }),
            "expected the 401 to survive, got {err:?}"
        );
    }

    #[tokio::test]
    async fn http_error_falls_back_to_cache_when_present() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/credits")
            .with_status(401)
            .with_body(r#"{"error":"unauthorized"}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/key")
            .with_status(401)
            .with_body(r#"{"error":"unauthorized"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        // Seed cache with a "snapshot" repr.
        let seed = serde_json::json!({
            "snapshot": {
                "label":"OpenRouter — seed","total_credits": 50.0,
                "total_usage": 10.0,"usage_daily":1.0,"usage_weekly":3.0,
                "usage_monthly":10.0,"is_free_tier":false,
                "limit":null,"limit_remaining":null
            }
        });
        cache.write_payload(seed.to_string().as_bytes()).unwrap();

        let client = reqwest::Client::new();
        let endpoints = Endpoints {
            credits: format!("{}/api/v1/credits", server.url()),
            key: format!("{}/api/v1/key", server.url()),
        };
        let out = fetch_snapshot(&client, "k", &cache, &endpoints, Duration::from_secs(0))
            .await
            .unwrap();
        assert!(out.stale);
        assert_eq!(out.snapshot.label, "OpenRouter — seed");
        assert_eq!(out.last_error.as_ref().map(|(c, _)| *c), Some(401));
    }
}
