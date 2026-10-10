//! OpenRouter fetch — combines `/api/v1/credits` and `/api/v1/key` under
//! the shared cache + flock primitives.

use std::time::Duration;

use crate::cache::{Cache, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::usage::OpenRouterSnapshot;

use super::types::{ActivityItem, CreditsData, KeyData, OrEnvelope, combine};

pub const BASE_URL: &str = "https://openrouter.ai/api/v1";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub credits: String,
    pub key: String,
    pub activity: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            credits: format!("{BASE_URL}/credits"),
            key: format!("{BASE_URL}/key"),
            activity: format!("{BASE_URL}/activity"),
        }
    }
}

/// This vendor's [`Outcome`](crate::outcome::Outcome) — the shared shape,
/// specialised to its snapshot.
pub type FetchOutcome = crate::outcome::Outcome<OpenRouterSnapshot>;

/// Cache-aware fetch. Mirrors `anthropic::fetch::fetch_snapshot` semantics:
/// fresh cache short-circuits; on failure, fall back to cache + mark stale.
/// `management_key` is the optional OpenRouter management key: only
/// `/activity` uses it, and when it is `None` that request is skipped
/// entirely instead of fired at a 401 with the inference key.
pub async fn fetch_snapshot(
    client: &reqwest::Client,
    api_key: &str,
    management_key: Option<&str>,
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

    match fetch_live(client, endpoints, api_key, management_key).await {
        Ok((credits, key, recent_models)) => {
            let snap = combine(credits, key, recent_models);
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
        recent_models: s
            .get("recent_models")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
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
        "recent_models": snap.recent_models,
    })
}

async fn fetch_live(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    api_key: &str,
    management_key: Option<&str>,
) -> Result<(CreditsData, KeyData, Vec<String>)> {
    // Fetch in parallel.
    let credits_fut = fetch_one::<CreditsData>(client, &endpoints.credits, api_key);
    let key_fut = fetch_one::<KeyData>(client, &endpoints.key, api_key);
    // `/activity` answers to a management key only, so without one the
    // request is not even fired — a guaranteed 401, and the regular key must
    // never leave for that path.
    let activity_fut = async {
        match management_key {
            Some(key) => {
                Some(fetch_one::<Vec<ActivityItem>>(client, &endpoints.activity, key).await)
            }
            None => None,
        }
    };
    let (credits, key, activity) = tokio::join!(credits_fut, key_fut, activity_fut);
    let recent_models = match activity {
        Some(Ok(items)) => extract_recent_models(items),
        _ => Vec::new(),
    };
    Ok((credits?, key?, recent_models))
}

fn shorten_model_id(name: &str) -> &str {
    let mut clean = name.strip_suffix("-instruct").unwrap_or(name);
    clean = clean.strip_suffix("-preview").unwrap_or(clean);
    clean = clean.strip_suffix("-chat").unwrap_or(clean);
    while clean.len() > 18 && clean.contains('-') {
        if let Some((prefix, _)) = clean.rsplit_once('-') {
            clean = prefix;
        } else {
            break;
        }
    }
    clean
}

fn extract_recent_models(mut items: Vec<ActivityItem>) -> Vec<String> {
    // "Recent" means recent: newest dates first. The sort is stable, so items
    // without a `date` (and same-date ties) keep the API's response order.
    items.sort_by(|a, b| b.date.cmp(&a.date));
    let mut models: Vec<(String, f64, u64)> = Vec::new();
    for item in items {
        if let Some(m) = item.model {
            if let Some(existing) = models.iter_mut().find(|(name, _, _)| name == &m) {
                existing.1 += item.usage;
                existing.2 += item.requests;
            } else {
                models.push((m, item.usage, item.requests));
            }
        }
    }
    models.truncate(2);
    models
        .into_iter()
        .map(|(m, spend, reqs)| {
            let short_name = m.split('/').next_back().unwrap_or(&m);
            let clean_name = shorten_model_id(short_name);
            let cost = crate::format::usd(spend);
            if spend > 0.0 {
                format!("{clean_name} ({cost} · {reqs} reqs)")
            } else {
                format!("{clean_name} ({reqs} reqs)")
            }
        })
        .collect()
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
            activity: format!("{}/api/v1/activity", server.url()),
        };
        let out = fetch_snapshot(
            &client,
            "sk-or-test",
            None,
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
            activity: format!("{}/api/v1/activity", server.url()),
        };
        let err = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-or-test",
            None,
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

    /// The fan-out behind `[[openrouter.accounts]]` (#221): every account is
    /// its own `fetch_snapshot` call with its own key and its own cache
    /// subdirectory (what `Cache::for_vendor_account` lays out in production),
    /// so two keys must land as two distinct snapshots that never share a
    /// payload — not even a stale one.
    #[tokio::test]
    async fn two_accounts_fan_out_to_distinct_entries_and_caches() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/credits")
            .match_header("authorization", "Bearer work-key")
            .with_status(200)
            .with_body(r#"{"data":{"total_credits":100.0,"total_usage":25.5}}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/key")
            .match_header("authorization", "Bearer work-key")
            .with_status(200)
            .with_body(
                r#"{"data":{"label":"work","limit":null,"limit_remaining":null,
                "usage":25.5,"usage_daily":1.0,"usage_weekly":7.0,"usage_monthly":25.5,
                "is_free_tier":false}}"#,
            )
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/credits")
            .match_header("authorization", "Bearer personal-key")
            .with_status(200)
            .with_body(r#"{"data":{"total_credits":40.0,"total_usage":4.0}}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/key")
            .match_header("authorization", "Bearer personal-key")
            .with_status(200)
            .with_body(
                r#"{"data":{"label":"home","limit":null,"limit_remaining":null,
                "usage":4.0,"usage_daily":0.5,"usage_weekly":2.0,"usage_monthly":4.0,
                "is_free_tier":true}}"#,
            )
            .create_async()
            .await;

        let td = tempfile::TempDir::new().unwrap();
        let endpoints = Endpoints {
            credits: format!("{}/api/v1/credits", server.url()),
            key: format!("{}/api/v1/key", server.url()),
            activity: format!("{}/api/v1/activity", server.url()),
        };
        let mut outcomes = Vec::new();
        let mut caches = Vec::new();
        for (label, key) in [("work", "work-key"), ("personal", "personal-key")] {
            // The same per-account subdirectory `Cache::for_vendor_account`
            // builds in production, created via the hermetic `Cache::at`.
            let cache = Cache::at(td.path().join("openrouter").join(label));
            cache.ensure_dir().unwrap();
            let outcome = fetch_snapshot(
                &reqwest::Client::new(),
                key,
                None,
                &cache,
                &endpoints,
                Duration::from_secs(0),
            )
            .await
            .unwrap();
            outcomes.push(outcome);
            caches.push(cache);
        }

        let [work, personal] = &outcomes[..] else {
            panic!("expected exactly two account outcomes");
        };
        assert_eq!(work.snapshot.label, "OpenRouter — work");
        assert!((work.snapshot.balance() - 74.5).abs() < 1e-9);
        assert_eq!(personal.snapshot.label, "OpenRouter — home");
        assert!((personal.snapshot.balance() - 36.0).abs() < 1e-9);
        assert_ne!(work.snapshot, personal.snapshot);

        // Each cache holds its own account's payload, and only its own.
        let work_bytes = caches[0].maybe_payload().unwrap().unwrap();
        let personal_bytes = caches[1].maybe_payload().unwrap().unwrap();
        assert_ne!(work_bytes, personal_bytes);
        assert_eq!(parse_cache(&work_bytes).unwrap().label, "OpenRouter — work");
        assert_eq!(
            parse_cache(&personal_bytes).unwrap().label,
            "OpenRouter — home"
        );
    }

    /// One account's dead key must not take the others down: the fan-out
    /// runs one fetch per account against one cache per account, so a 401
    /// stays inside the entry it belongs to (#221).
    #[tokio::test]
    async fn a_401_on_one_account_does_not_fail_the_other() {
        let mut server = mockito::Server::new_async().await;
        for path in ["/api/v1/credits", "/api/v1/key"] {
            server
                .mock("GET", path)
                .match_header("authorization", "Bearer revoked-key")
                .with_status(401)
                .with_body(r#"{"error":"unauthorized"}"#)
                .create_async()
                .await;
        }
        server
            .mock("GET", "/api/v1/credits")
            .match_header("authorization", "Bearer live-key")
            .with_status(200)
            .with_body(r#"{"data":{"total_credits":30.0,"total_usage":6.0}}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/key")
            .match_header("authorization", "Bearer live-key")
            .with_status(200)
            .with_body(
                r#"{"data":{"label":"live","limit":null,"limit_remaining":null,
                "usage":6.0,"usage_daily":2.0,"usage_weekly":4.0,"usage_monthly":6.0,
                "is_free_tier":false}}"#,
            )
            .create_async()
            .await;

        let td = tempfile::TempDir::new().unwrap();
        let endpoints = Endpoints {
            credits: format!("{}/api/v1/credits", server.url()),
            key: format!("{}/api/v1/key", server.url()),
            activity: format!("{}/api/v1/activity", server.url()),
        };
        let client = reqwest::Client::new();

        let revoked_cache = Cache::at(td.path().join("openrouter").join("revoked"));
        revoked_cache.ensure_dir().unwrap();
        let revoked = fetch_snapshot(
            &client,
            "revoked-key",
            None,
            &revoked_cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await;
        assert!(
            matches!(revoked, Err(AppError::Http { status: 401, .. })),
            "expected the revoked account to fail with its own 401, got {revoked:?}"
        );

        let live_cache = Cache::at(td.path().join("openrouter").join("live"));
        live_cache.ensure_dir().unwrap();
        let live = fetch_snapshot(
            &client,
            "live-key",
            None,
            &live_cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert_eq!(live.snapshot.label, "OpenRouter — live");
        assert!((live.snapshot.balance() - 24.0).abs() < 1e-9);
        assert!(
            live_cache.maybe_payload().unwrap().is_some(),
            "the healthy account's cache must still be written"
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
            activity: format!("{}/api/v1/activity", server.url()),
        };
        let out = fetch_snapshot(
            &client,
            "k",
            None,
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert!(out.stale);
        assert_eq!(out.snapshot.label, "OpenRouter — seed");
        assert_eq!(out.last_error.as_ref().map(|(c, _)| *c), Some(401));
    }

    /// Shared fixtures for the `/activity` tests: healthy credits + key
    /// endpoints, so the only variable is the activity call itself.
    async fn mock_credits_and_key(server: &mut mockito::ServerGuard) {
        server
            .mock("GET", "/api/v1/credits")
            .match_header("authorization", "Bearer sk-or-test")
            .with_status(200)
            .with_body(r#"{"data":{"total_credits":100.0,"total_usage":25.5}}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/key")
            .match_header("authorization", "Bearer sk-or-test")
            .with_status(200)
            .with_body(
                r#"{"data":{"label":"prod","limit":null,"limit_remaining":null,
                "usage":25.5,"usage_daily":1.0,"usage_weekly":7.0,"usage_monthly":25.5,
                "is_free_tier":false}}"#,
            )
            .create_async()
            .await;
    }

    fn endpoints_for(server: &mockito::ServerGuard) -> Endpoints {
        Endpoints {
            credits: format!("{}/api/v1/credits", server.url()),
            key: format!("{}/api/v1/key", server.url()),
            activity: format!("{}/api/v1/activity", server.url()),
        }
    }

    #[tokio::test]
    async fn activity_feeds_recent_models_with_the_management_key() {
        let mut server = mockito::Server::new_async().await;
        mock_credits_and_key(&mut server).await;
        let activity = server
            .mock("GET", "/api/v1/activity")
            // The management key goes to /activity — the regular inference
            // key must never be sent there.
            .match_header("authorization", "Bearer sk-or-mgmt")
            .with_status(200)
            .with_body(
                r#"{"data":[
                {"date":"2026-10-02","model":"openai/gpt-5-codex","usage":1.25,"requests":42},
                {"date":"2026-10-01","model":"anthropic/claude-sonnet-4.5","usage":0.5,"requests":7}
                ]}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-or-test",
            Some("sk-or-mgmt"),
            &cache,
            &endpoints_for(&server),
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert_eq!(
            out.snapshot.recent_models,
            vec![
                "gpt-5-codex ($1.25 · 42 reqs)".to_string(),
                "claude-sonnet-4.5 ($0.50 · 7 reqs)".to_string(),
            ]
        );
        activity.assert_async().await;
    }

    #[tokio::test]
    async fn malformed_activity_leaves_recent_models_empty() {
        let mut server = mockito::Server::new_async().await;
        mock_credits_and_key(&mut server).await;
        server
            .mock("GET", "/api/v1/activity")
            .with_status(200)
            .with_body(r#"{"data":"not-an-array"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-or-test",
            Some("sk-or-mgmt"),
            &cache,
            &endpoints_for(&server),
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert!(out.snapshot.recent_models.is_empty());
        assert!((out.snapshot.balance() - 74.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn a_401_from_activity_leaves_recent_models_empty() {
        let mut server = mockito::Server::new_async().await;
        mock_credits_and_key(&mut server).await;
        server
            .mock("GET", "/api/v1/activity")
            .with_status(401)
            .with_body(r#"{"error":"unauthorized"}"#)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-or-test",
            Some("sk-or-mgmt"),
            &cache,
            &endpoints_for(&server),
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert!(out.snapshot.recent_models.is_empty());
        assert!((out.snapshot.balance() - 74.5).abs() < 1e-9);
    }

    /// Without a management key the activity endpoint only ever answers 401,
    /// so the request must not be fired at all — zero hits on the mock.
    #[tokio::test]
    async fn without_a_management_key_the_activity_request_is_not_fired() {
        let mut server = mockito::Server::new_async().await;
        mock_credits_and_key(&mut server).await;
        let activity = server
            .mock("GET", "/api/v1/activity")
            .expect(0)
            .create_async()
            .await;

        let (_td, cache) = cache_fixture();
        let out = fetch_snapshot(
            &reqwest::Client::new(),
            "sk-or-test",
            None,
            &cache,
            &endpoints_for(&server),
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert!(out.snapshot.recent_models.is_empty());
        assert!((out.snapshot.balance() - 74.5).abs() < 1e-9);
        activity.assert_async().await;
    }

    fn activity_item(
        date: Option<&str>,
        model: Option<&str>,
        usage: f64,
        requests: u64,
    ) -> ActivityItem {
        ActivityItem {
            date: date.map(str::to_string),
            model: model.map(str::to_string),
            usage,
            requests,
        }
    }

    #[test]
    fn recent_models_sort_by_date_descending_before_deduping() {
        let items = vec![
            activity_item(Some("2026-09-28"), Some("a/old-model"), 0.10, 3),
            activity_item(Some("2026-10-02"), Some("b/new-model"), 0.20, 5),
            // The older first occurrence must not win: after sorting, the
            // 2026-10-01 row merges into old-model's first (newest) slot.
            activity_item(Some("2026-10-01"), Some("a/old-model"), 0.30, 2),
        ];
        let models = extract_recent_models(items);
        assert_eq!(
            models,
            vec![
                "new-model ($0.20 · 5 reqs)".to_string(),
                "old-model ($0.40 · 5 reqs)".to_string(),
            ]
        );
    }

    #[test]
    fn recent_models_without_dates_keep_response_order() {
        let items = vec![
            activity_item(None, Some("a/first"), 0.0, 1),
            activity_item(None, Some("b/second"), 0.0, 2),
            activity_item(None, Some("c/third"), 0.0, 3),
        ];
        let models = extract_recent_models(items);
        assert_eq!(
            models,
            vec!["first (1 reqs)".to_string(), "second (2 reqs)".to_string()]
        );
    }

    #[test]
    fn shorten_model_id_cases() {
        assert_eq!(shorten_model_id("llama-3.3-70b-instruct"), "llama-3.3-70b");
        assert_eq!(shorten_model_id("gpt-5-preview"), "gpt-5");
        assert_eq!(shorten_model_id("some-model-chat"), "some-model");
        assert_eq!(
            shorten_model_id("claude-3-5-sonnet-20241022"),
            "claude-3-5-sonnet"
        );
        assert_eq!(
            shorten_model_id("a-very-long-model-name-with-many-parts"),
            "a-very-long-model"
        );
        assert_eq!(shorten_model_id("gpt"), "gpt");
    }
}
