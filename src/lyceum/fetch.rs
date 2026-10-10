//! Lyceum billing-credit balance fetcher.
use super::types::Credits;
use crate::cache::{Cache, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::usage::{LyceumSnapshot, finite_amount};
use crate::vendor::{MAX_BODY_BYTES, read_body_capped};
use std::time::Duration;

pub const BASE_URL: &str = "https://api.lyceum.technology";
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub credits: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            credits: format!("{BASE_URL}/api/v2/external/billing/credits"),
        }
    }
}
pub type FetchOutcome = crate::outcome::Outcome<LyceumSnapshot>;

pub async fn fetch_snapshot(
    client: &reqwest::Client,
    api_key: &str,
    cache: &Cache,
    ttl: Duration,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;
    if let Some(bytes) = cache.fresh_payload(ttl)?
        && let Ok(snapshot) = parse_cache(&bytes)
    {
        return Ok(crate::outcome::Outcome::cached(snapshot, cache, false));
    }
    match fetch_live(client, api_key, &Endpoints::default()).await {
        Ok(snapshot) => {
            cache.write_payload(&serde_json::to_vec(
                &serde_json::json!({"snapshot": snapshot}),
            )?)?;
            Ok(crate::outcome::Outcome::fresh(snapshot))
        }
        Err(e) if e.is_transient() => crate::outcome::fallback(cache, None, e, parse_cache),
        Err(e) => {
            cache.mark_stale();
            let status = match &e {
                AppError::Http { status, .. } => *status,
                _ => 0,
            };
            let safe = match &e {
                AppError::Http { status, .. } => AppError::Http {
                    status: *status,
                    body: "Lyceum request failed".into(),
                },
                _ => AppError::Other("Lyceum request failed".into()),
            };
            let diag = cache.write_last_error(status, &safe.to_string());
            crate::outcome::fallback(cache, Some(diag), safe, parse_cache)
        }
    }
}

fn parse_cache(bytes: &[u8]) -> Result<LyceumSnapshot> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let snapshot = value
        .get("snapshot")
        .ok_or_else(|| AppError::Schema("lyceum cache missing snapshot".into()))?;
    let field = |name: &str| -> Result<f64> {
        let n = snapshot
            .get(name)
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| AppError::Schema(format!("lyceum cache missing {name}")))?;
        finite_amount("lyceum cache", name, n)
    };
    Ok(LyceumSnapshot {
        available_credits: field("available_credits")?,
        used_credits: field("used_credits")?,
        total_credits_used: field("total_credits_used")?,
        remaining_credits: field("remaining_credits")?,
        monthly_free_credits: field("monthly_free_credits")?,
        purchased_credits: field("purchased_credits")?,
    })
}

async fn fetch_live(
    client: &reqwest::Client,
    key: &str,
    endpoints: &Endpoints,
) -> Result<LyceumSnapshot> {
    let response = tokio::time::timeout(
        HTTP_TIMEOUT,
        client
            .get(&endpoints.credits)
            .bearer_auth(key)
            .header("Accept", "application/json")
            .send(),
    )
    .await
    .map_err(|_| AppError::Transport("lyceum request timed out".into()))??;
    let status = response.status();
    let body = read_body_capped(response, MAX_BODY_BYTES).await?;
    if !status.is_success() {
        return Err(AppError::Http {
            status: status.as_u16(),
            body: "Lyceum billing request failed".into(),
        });
    }
    let response: Credits = serde_json::from_slice(&body)
        .map_err(|_| AppError::Schema("lyceum: invalid credits response".into()))?;
    response.into_snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    #[tokio::test]
    async fn request_uses_bearer_and_parses_credit_balance() {
        let mut server = mockito::Server::new_async().await;
        let mock = server.mock("GET", "/api/v2/external/billing/credits").match_header("authorization", "Bearer synthetic-key").with_status(200).with_body(r#"{"available_credits":13.5,"used_credits":2.5,"total_credits_used":2.5,"remaining_credits":13.5,"monthly_free_credits":0,"purchased_credits":10,"extra":true}"#).create_async().await;
        let endpoint = Endpoints {
            credits: format!("{}/api/v2/external/billing/credits", server.url()),
        };
        let got = fetch_live(&reqwest::Client::new(), "synthetic-key", &endpoint)
            .await
            .unwrap();
        mock.assert();
        assert_eq!(got.available_credits, 13.5);
    }
    #[tokio::test]
    async fn unauthorized_response_does_not_echo_body() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/v2/external/billing/credits")
            .with_status(401)
            .with_body("private synthetic credential")
            .create_async()
            .await;
        let endpoint = Endpoints {
            credits: format!("{}/api/v2/external/billing/credits", server.url()),
        };
        let err = fetch_live(&reqwest::Client::new(), "synthetic-key", &endpoint)
            .await
            .unwrap_err();
        assert!(!err.to_string().contains("private synthetic credential"));
        mock.assert();
    }
    #[test]
    fn cache_round_trips_balance_only_snapshot() {
        let td = TempDir::new().unwrap();
        let cache = Cache::at(td.path().join("lyceum"));
        cache.ensure_dir().unwrap();
        let raw = serde_json::to_vec(&serde_json::json!({"snapshot":{"available_credits":10.0,"used_credits":1.0,"total_credits_used":1.0,"remaining_credits":10.0,"monthly_free_credits":0.0,"purchased_credits":10.0}})).unwrap();
        let parsed = parse_cache(&raw).unwrap();
        assert_eq!(parsed.remaining_credits, 10.0);
        let broken = br#"{"snapshot":{"available_credits":10}}"#;
        assert!(parse_cache(broken).is_err());
    }
}
