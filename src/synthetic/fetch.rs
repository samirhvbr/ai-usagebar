use std::time::Duration;

use crate::cache::{Cache, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::usage::SyntheticSnapshot;
use crate::vendor::{MAX_BODY_BYTES, read_body_capped};

use super::types::Response;

pub const BASE_URL: &str = "https://api.synthetic.new";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);

pub type FetchOutcome = crate::outcome::Outcome<SyntheticSnapshot>;

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub quotas: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            quotas: format!("{BASE_URL}/v2/quotas"),
        }
    }
}

pub async fn fetch_snapshot(
    client: &reqwest::Client,
    api_key: &str,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
    plan_tier: Option<&str>,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;
    let parse_cache = |bytes: &[u8]| -> Result<SyntheticSnapshot> {
        let response: Response = serde_json::from_slice(bytes)
            .map_err(|_| AppError::Schema("Synthetic quota cache is invalid".into()))?;
        Ok(named(response.snapshot()?, plan_tier))
    };

    if let Some(bytes) = cache.fresh_payload(cache_ttl)?
        && let Ok(snapshot) = parse_cache(&bytes)
    {
        return Ok(crate::outcome::Outcome::cached(snapshot, cache, false));
    }
    // A cache written before the weekly-only quota change is incompatible
    // with this parser, but rejecting it must not bypass an active backoff.
    // Keep usable weekly figures above; otherwise wait before refetching.
    if let Some(remaining) = cache.backoff_remaining() {
        return Err(AppError::Http {
            status: 429,
            body: format!(
                "rate limited; next attempt in {}",
                crate::cache::human_backoff(remaining)
            ),
        });
    }

    match request(client, api_key, endpoints).await {
        Ok((response, snapshot)) => {
            cache.write_payload(&serde_json::to_vec(&response)?)?;
            Ok(crate::outcome::Outcome::fresh(named(snapshot, plan_tier)))
        }
        Err(error) if error.is_transient() => {
            crate::outcome::fallback(cache, None, error, parse_cache)
        }
        Err(error) => {
            let code = match &error {
                AppError::Http { status, .. } => *status,
                _ => 0,
            };
            cache.mark_stale();
            let diagnostic = cache.write_last_error(code, &error.to_string());
            crate::outcome::fallback(cache, Some(diagnostic), error, parse_cache)
        }
    }
}

/// Attach the configured plan tier label to a parsed wire snapshot.
fn named(snapshot: SyntheticSnapshot, plan_tier: Option<&str>) -> SyntheticSnapshot {
    SyntheticSnapshot {
        plan: plan_tier.map(str::to_string),
        ..snapshot
    }
}

async fn request(
    client: &reqwest::Client,
    api_key: &str,
    endpoints: &Endpoints,
) -> Result<(Response, SyntheticSnapshot)> {
    let response = tokio::time::timeout(
        HTTP_TIMEOUT,
        client.get(&endpoints.quotas).bearer_auth(api_key).send(),
    )
    .await
    .map_err(|_| AppError::Transport("Synthetic quota request timed out".into()))??;
    let status = response.status();
    let bytes = read_body_capped(response, MAX_BODY_BYTES).await?;
    if !status.is_success() {
        return Err(AppError::Http {
            status: status.as_u16(),
            body: String::from_utf8_lossy(&bytes).chars().take(200).collect(),
        });
    }
    let response: Response = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Schema("Synthetic quota response schema mismatch".into()))?;
    let snapshot = response.snapshot()?;
    Ok((response, snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cache() -> (TempDir, Cache) {
        let dir = TempDir::new().unwrap();
        let cache = Cache::at(dir.path().join("synthetic"));
        cache.ensure_dir().unwrap();
        (dir, cache)
    }

    const PLAN_BODY: &str = r#"{"planWeeklyLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":80,"nextRegenPercent":2}}"#;

    #[tokio::test]
    async fn legacy_cache_respects_backoff_and_refetches_after_expiry() {
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/v2/quotas")
            .with_status(200)
            .with_body(PLAN_BODY)
            .expect(1)
            .create_async()
            .await;
        let endpoints = Endpoints {
            quotas: format!("{}/v2/quotas", server.url()),
        };
        let (_dir, cache) = cache();
        // v1.44 stored only the retired five-hour subscription object.
        let legacy =
            br#"{"subscription":{"limit":135,"requests":27,"renewsAt":"2026-11-01T00:00:00Z"}}"#;
        cache.write_payload(legacy).unwrap();
        let diagnostic = cache.write_last_error(429, "rate limited");
        let retry_after = std::fs::read(cache.retry_after_path()).unwrap();
        let client = reqwest::Client::new();

        let error = fetch_snapshot(
            &client,
            "syn-test",
            &cache,
            &endpoints,
            Duration::ZERO,
            None,
        )
        .await
        .expect_err("an incompatible cache must not bypass the active backoff");
        assert!(matches!(
            error,
            AppError::Http { status: 429, body } if body.contains("next attempt in")
        ));
        assert_eq!(std::fs::read(cache.payload_path()).unwrap(), legacy);
        assert_eq!(
            std::fs::read(cache.retry_after_path()).unwrap(),
            retry_after
        );
        assert_eq!(cache.read_last_error(), Some(diagnostic));

        // Expire the persisted deadline without sleeping through five minutes.
        std::fs::write(cache.retry_after_path(), b"1").unwrap();
        let outcome = fetch_snapshot(
            &client,
            "syn-test",
            &cache,
            &endpoints,
            Duration::ZERO,
            None,
        )
        .await
        .unwrap();
        assert!(outcome.off_the_wire());
        assert_eq!(outcome.snapshot.weekly.utilization_pct, 20);
        assert!(cache.backoff_remaining().is_none());
        assert!(cache.read_last_error().is_none());
        request.assert_async().await;
    }

    #[tokio::test]
    async fn usable_weekly_cache_stays_visible_during_backoff() {
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/v2/quotas")
            .expect(0)
            .create_async()
            .await;
        let (_dir, cache) = cache();
        cache.write_payload(PLAN_BODY.as_bytes()).unwrap();
        let diagnostic = cache.write_last_error(429, "rate limited");

        let outcome = fetch_snapshot(
            &reqwest::Client::new(),
            "syn-test",
            &cache,
            &Endpoints {
                quotas: format!("{}/v2/quotas", server.url()),
            },
            Duration::from_secs(60),
            Some("Standard"),
        )
        .await
        .unwrap();

        assert_eq!(outcome.snapshot.weekly.utilization_pct, 20);
        assert_eq!(outcome.snapshot.plan.as_deref(), Some("Standard"));
        assert!(outcome.stale);
        assert!(!outcome.off_the_wire());
        assert_eq!(outcome.last_error, Some(diagnostic));
        assert!(cache.backoff_remaining().is_some());
        request.assert_async().await;
    }

    #[tokio::test]
    async fn sends_bearer_key_and_parses_documented_shape() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/v2/quotas")
            .match_header("authorization", "Bearer syn-test")
            .with_status(200)
            .with_body(PLAN_BODY)
            .create_async()
            .await;
        let (_dir, cache) = cache();
        let outcome = fetch_snapshot(
            &reqwest::Client::new(),
            "syn-test",
            &cache,
            &Endpoints {
                quotas: format!("{}/v2/quotas", server.url()),
            },
            Duration::ZERO,
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.snapshot.weekly.utilization_pct, 20);
        assert_eq!(outcome.snapshot.plan, None);
        assert!(!outcome.stale);
    }

    #[tokio::test]
    async fn carries_the_configured_plan_tier_label() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/v2/quotas")
            .with_status(200)
            .with_body(PLAN_BODY)
            .create_async()
            .await;
        let (_dir, cache) = cache();
        let outcome = fetch_snapshot(
            &reqwest::Client::new(),
            "syn-test",
            &cache,
            &Endpoints {
                quotas: format!("{}/v2/quotas", server.url()),
            },
            Duration::ZERO,
            Some("Standard"),
        )
        .await
        .unwrap();
        assert_eq!(outcome.snapshot.plan.as_deref(), Some("Standard"));
    }

    const INVALID_QUOTAS: [&str; 3] = [
        r#"{"planWeeklyLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":-1,"nextRegenPercent":2}}"#,
        r#"{"planWeeklyLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":80,"nextRegenPercent":-2}}"#,
        r#"{"planWeeklyLimit":{"nextRegenAt":"not-a-time","percentRemaining":80,"nextRegenPercent":2}}"#,
    ];

    #[tokio::test]
    async fn invalid_quota_values_keep_the_last_good_snapshot_stale() {
        for body in INVALID_QUOTAS {
            let mut server = mockito::Server::new_async().await;
            let request = server
                .mock("GET", "/v2/quotas")
                .with_status(200)
                .with_body(body)
                .create_async()
                .await;
            let (_dir, cache) = cache();
            let good = PLAN_BODY.as_bytes();
            cache.write_payload(good).unwrap();

            let outcome = fetch_snapshot(
                &reqwest::Client::new(),
                "syn-test",
                &cache,
                &Endpoints {
                    quotas: format!("{}/v2/quotas", server.url()),
                },
                Duration::ZERO,
                None,
            )
            .await
            .expect("invalid wire values should fall back to the good cache");

            assert_eq!(outcome.snapshot.weekly.utilization_pct, 20);
            assert!(outcome.stale);
            assert!(!outcome.off_the_wire());
            let (code, message) = outcome.last_error.expect("schema diagnostic");
            assert_eq!(code, 0);
            assert!(message.contains("schema mismatch"));
            assert_eq!(std::fs::read(cache.payload_path()).unwrap(), good);
            assert!(cache.is_stale());
            request.assert_async().await;
        }
    }

    #[tokio::test]
    async fn invalid_quota_values_without_cache_record_the_schema_failure() {
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/v2/quotas")
            .with_status(200)
            .with_body(INVALID_QUOTAS[0])
            .create_async()
            .await;
        let (_dir, cache) = cache();

        let error = fetch_snapshot(
            &reqwest::Client::new(),
            "syn-test",
            &cache,
            &Endpoints {
                quotas: format!("{}/v2/quotas", server.url()),
            },
            Duration::ZERO,
            None,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, AppError::Schema(_)));
        assert!(!cache.payload_path().exists());
        assert!(cache.is_stale());
        assert_eq!(cache.read_last_error(), Some((0, error.to_string())));
        request.assert_async().await;
    }

    #[tokio::test]
    async fn auth_body_is_redacted_when_warm_cache_is_used() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/v2/quotas")
            .with_status(401)
            .with_body("secret@example.test credential=canary")
            .create_async()
            .await;
        let (_dir, cache) = cache();
        cache
            .write_payload(
                br#"{"planWeeklyLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":90,"nextRegenPercent":2}}"#,
            )
            .unwrap();
        let outcome = fetch_snapshot(
            &reqwest::Client::new(),
            "syn-test",
            &cache,
            &Endpoints {
                quotas: format!("{}/v2/quotas", server.url()),
            },
            Duration::ZERO,
            None,
        )
        .await
        .unwrap();
        assert!(outcome.stale);
        let (_, message) = outcome.last_error.unwrap();
        assert_eq!(message, crate::error::AUTH_FAILURE_MESSAGE);
        assert!(!message.contains("canary"));
    }
}
