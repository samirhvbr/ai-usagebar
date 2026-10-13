//! Fetch the ClinePass plan and its three usage limits.
//!
//! A refresh is two small calls: the plan (name, billing period) and
//! `usage-limits`, where Cline itself reports how much of each window is used
//! and when it resets. The windows are not recomputed here — see
//! [`super::types`] for why that approach was dropped.

use std::fmt::Write as _;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cache::{Cache, acquire_lock_async};
use crate::error::{AUTH_FAILURE_MESSAGE, AppError, Result};
use crate::vendor::{MAX_BODY_BYTES, read_body_capped};

use super::creds::Credential;
use super::types::{Plan, Snapshot, Window, parse_limits, parse_plan};

pub const BASE_URL: &str = "https://api.cline.bot";

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
const SCHEMA_ERROR: &str = "Cline response schema mismatch";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            base: BASE_URL.to_string(),
        }
    }
}

impl Endpoints {
    fn me(&self) -> String {
        format!("{}/api/v1/users/me", self.base)
    }

    fn plan(&self) -> String {
        format!("{}/api/v1/users/me/plan", self.base)
    }

    fn usage_limits(&self) -> String {
        format!("{}/api/v1/users/me/plan/usage-limits", self.base)
    }
}

/// This vendor's [`Outcome`](crate::outcome::Outcome).
pub type FetchOutcome = crate::outcome::Outcome<Snapshot>;

/// What the cache holds between refreshes: the last answer, as the server
/// gave it. Windows are aged to the current instant when it is read back.
#[derive(Debug, Clone, Default)]
struct State {
    plan: Plan,
    windows: [Option<Window>; 3],
}

impl State {
    fn snapshot(&self, now: DateTime<Utc>) -> Snapshot {
        let [five_hour, seven_day, thirty_day] = self
            .windows
            .clone()
            .map(|window| window.map(|window| window.aged(now)));
        Snapshot {
            plan: self.plan.name.clone(),
            five_hour,
            seven_day,
            thirty_day,
            period_end: self.plan.period_end,
        }
    }
}

pub async fn fetch_snapshot(
    client: &reqwest::Client,
    credential: &Credential,
    cache: &Cache,
    endpoints: &Endpoints,
    ttl: Duration,
    now: DateTime<Utc>,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;

    let user_id = match credential.account_id.clone() {
        Some(id) => id,
        None if credential.expired => {
            return Err(AppError::Credentials(super::creds::EXPIRED.to_string()));
        }
        None => fetch_user_id(client, endpoints, &credential.token).await?,
    };
    let target = target_key(endpoints, &user_id);

    // A lapsed token would only earn a 401. The stored answer is still worth
    // showing, aged to the current instant: a window whose reset has passed
    // reads as empty, exactly as it does at Cline.
    if credential.expired {
        cache.mark_stale();
        let recorded = cache.write_last_error(401, super::creds::EXPIRED);
        return fallback_or_error(
            cache,
            Some(recorded),
            &target,
            AppError::Credentials(super::creds::EXPIRED.to_string()),
            now,
        );
    }

    if cache.fresh_payload(ttl)?.is_some()
        && let Some(state) = cache
            .maybe_payload()?
            .and_then(|bytes| parse_cache(&bytes, &target).ok())
    {
        return Ok(crate::outcome::Outcome::cached(
            state.snapshot(now),
            cache,
            false,
        ));
    }

    match fetch_live(client, endpoints, &credential.token).await {
        Ok(state) => {
            cache.write_payload(&serde_json::to_vec(&state_repr(&target, &state))?)?;
            Ok(crate::outcome::Outcome::fresh(state.snapshot(now)))
        }
        Err(error @ AppError::Transport(_)) => fallback_or_error(cache, None, &target, error, now),
        Err(AppError::Http { status, .. }) => {
            let message = status_message(status).to_string();
            cache.mark_stale();
            cache.write_last_error(status, &message);
            fallback_or_error(
                cache,
                Some((status, message.clone())),
                &target,
                AppError::Http {
                    status,
                    body: message,
                },
                now,
            )
        }
        Err(AppError::Schema(_)) => {
            let message = SCHEMA_ERROR.to_string();
            cache.mark_stale();
            cache.write_last_error(0, &message);
            fallback_or_error(
                cache,
                Some((0, message.clone())),
                &target,
                AppError::Schema(message),
                now,
            )
        }
        Err(error) => fallback_or_error(cache, None, &target, error, now),
    }
}

async fn fetch_live(client: &reqwest::Client, endpoints: &Endpoints, token: &str) -> Result<State> {
    let schema = |_| AppError::Schema(SCHEMA_ERROR.to_string());
    let plan = parse_plan(&get_json(client, &endpoints.plan(), token).await?).map_err(schema)?;
    let windows =
        parse_limits(&get_json(client, &endpoints.usage_limits(), token).await?).map_err(schema)?;
    Ok(State { plan, windows })
}

/// Resolve the signed-in user id when the credential store did not record it.
async fn fetch_user_id(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    token: &str,
) -> Result<String> {
    let value = get_json(client, &endpoints.me(), token).await?;
    value
        .get("data")
        .unwrap_or(&value)
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| AppError::Schema(SCHEMA_ERROR.to_string()))
}

async fn get_json(client: &reqwest::Client, url: &str, token: &str) -> Result<Value> {
    let response = tokio::time::timeout(
        HTTP_TIMEOUT,
        client
            .get(url)
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send(),
    )
    .await
    .map_err(|_| AppError::Transport("Cline request timed out".to_string()))??;

    let status = response.status();
    let body = read_body_capped(response, MAX_BODY_BYTES).await?;
    if !status.is_success() {
        return Err(AppError::Http {
            status: status.as_u16(),
            body: status_message(status.as_u16()).to_string(),
        });
    }
    serde_json::from_slice(&body).map_err(|_| AppError::Schema(SCHEMA_ERROR.to_string()))
}

/// Stable, non-secret identity for the endpoint and account. The access token
/// rotates every few hours, so it cannot be what a cache is keyed on; the user
/// id it resolves to is what the cached figures actually belong to.
fn target_key(endpoints: &Endpoints, user_id: &str) -> String {
    let digest = Sha256::digest(user_id.as_bytes());
    let mut fingerprint = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(fingerprint, "{byte:02x}");
    }
    // `v3` marks the usage-limits schema. A `v2` cache holds a charge ledger
    // and no windows; it simply fails the target match and is refetched.
    format!("{}|user:{fingerprint}|v3", endpoints.base)
}

fn state_repr(target: &str, state: &State) -> Value {
    let window = |window: &Option<Window>| {
        window.as_ref().map(|window| {
            serde_json::json!({
                "percent": window.percent,
                "reset": window.reset.map(|at| at.to_rfc3339()),
            })
        })
    };
    serde_json::json!({
        "target": target,
        "plan": {
            "name": state.plan.name,
            "period_end": state.plan.period_end.map(|at| at.to_rfc3339()),
        },
        "windows": state.windows.iter().map(window).collect::<Vec<_>>(),
    })
}

fn parse_cache(body: &[u8], target: &str) -> Result<State> {
    let invalid = || AppError::Schema("Cline cache is invalid".to_string());
    let timestamp = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .and_then(|at| at.parse::<DateTime<chrono::FixedOffset>>().ok())
            .map(|at| at.with_timezone(&Utc))
    };
    let value: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    if value.get("target").and_then(Value::as_str) != Some(target) {
        return Err(AppError::Schema(
            "Cline cache belongs to a different account".to_string(),
        ));
    }
    let plan = value.get("plan").ok_or_else(invalid)?;
    let rows = value
        .get("windows")
        .and_then(Value::as_array)
        .filter(|rows| rows.len() == 3)
        .ok_or_else(invalid)?;

    // A listed window must carry its percentage: a half-written cache is
    // refetched, not rendered as 0 %.
    let mut windows: [Option<Window>; 3] = [None, None, None];
    for (slot, row) in windows.iter_mut().zip(rows) {
        if row.is_null() {
            continue;
        }
        *slot = Some(Window {
            percent: row
                .get("percent")
                .and_then(Value::as_f64)
                .filter(|percent| percent.is_finite())
                .ok_or_else(invalid)?,
            reset: timestamp(row.get("reset")),
        });
    }

    Ok(State {
        plan: Plan {
            name: plan.get("name").and_then(Value::as_str).map(str::to_string),
            period_end: timestamp(plan.get("period_end")),
        },
        windows,
    })
}

fn status_message(status: u16) -> &'static str {
    match status {
        401 | 403 => AUTH_FAILURE_MESSAGE,
        429 => "Cline request was rate limited",
        500..=599 => "Cline service is temporarily unavailable",
        _ => "Cline request failed",
    }
}

fn fallback_or_error(
    cache: &Cache,
    last_error: Option<(u16, String)>,
    target: &str,
    error: AppError,
    now: DateTime<Utc>,
) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, last_error, error, |body| {
        parse_cache(body, target).map(|state| state.snapshot(now))
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const PLAN_BODY: &str = include_str!("../../tests/fixtures/cline/plan.json");
    const LIMITS_BODY: &str = include_str!("../../tests/fixtures/cline/usage-limits.json");

    fn now() -> DateTime<Utc> {
        "2026-10-09T10:00:00Z".parse().expect("timestamp")
    }

    fn credential() -> Credential {
        Credential {
            token: "workos:test-token".to_string(),
            account_id: Some("usr-1".to_string()),
            source: "test".to_string(),
            expired: false,
        }
    }

    fn cache_fixture() -> (TempDir, Cache) {
        let dir = TempDir::new().expect("temporary cache directory");
        let cache = Cache::at(dir.path().join("cline"));
        cache.ensure_dir().expect("cache directory");
        (dir, cache)
    }

    async fn refresh(
        cache: &Cache,
        endpoints: &Endpoints,
        credential: &Credential,
        ttl: Duration,
        at: DateTime<Utc>,
    ) -> Result<FetchOutcome> {
        fetch_snapshot(
            &reqwest::Client::new(),
            credential,
            cache,
            endpoints,
            ttl,
            at,
        )
        .await
    }

    #[tokio::test]
    async fn reports_the_limits_exactly_as_the_server_states_them() {
        let mut server = mockito::Server::new_async().await;
        let plan = server
            .mock("GET", "/api/v1/users/me/plan")
            .match_header("authorization", "Bearer workos:test-token")
            .match_header("accept", "application/json")
            .with_body(PLAN_BODY)
            .create_async()
            .await;
        let limits = server
            .mock("GET", "/api/v1/users/me/plan/usage-limits")
            .match_header("authorization", "Bearer workos:test-token")
            .with_body(LIMITS_BODY)
            .create_async()
            .await;
        let (_dir, cache) = cache_fixture();

        let output = refresh(
            &cache,
            &Endpoints { base: server.url() },
            &credential(),
            Duration::from_secs(60),
            now(),
        )
        .await
        .expect("a successful refresh");

        let snapshot = output.snapshot;
        assert_eq!(snapshot.plan.as_deref(), Some("Cline Pass (Monthly)"));
        assert_eq!(snapshot.five_hour.as_ref().expect("5h").pct(), 0);
        assert_eq!(snapshot.seven_day.as_ref().expect("weekly").pct(), 5);
        assert_eq!(snapshot.thirty_day.as_ref().expect("monthly").pct(), 2);
        assert!(!output.stale);
        plan.assert_async().await;
        limits.assert_async().await;
    }

    #[tokio::test]
    async fn a_fresh_cache_is_aged_rather_than_refetched() {
        let mut server = mockito::Server::new_async().await;
        let plan = server
            .mock("GET", "/api/v1/users/me/plan")
            .with_body(PLAN_BODY)
            .expect(1)
            .create_async()
            .await;
        let limits = server
            .mock("GET", "/api/v1/users/me/plan/usage-limits")
            .with_body(LIMITS_BODY)
            .expect(1)
            .create_async()
            .await;
        let (_dir, cache) = cache_fixture();
        let endpoints = Endpoints { base: server.url() };
        let ttl = Duration::from_secs(600);
        refresh(&cache, &endpoints, &credential(), ttl, now())
            .await
            .expect("first refresh");

        // Past the weekly reset (2026-10-10T13:41Z) the cached read says the
        // week is empty again, without touching the network.
        let later = now() + chrono::Duration::days(2);
        let output = refresh(&cache, &endpoints, &credential(), ttl, later)
            .await
            .expect("cached refresh");

        let week = output.snapshot.seven_day.expect("weekly");
        assert_eq!(week.pct(), 0);
        assert_eq!(week.resets_at(), None);
        assert_eq!(output.snapshot.thirty_day.expect("monthly").pct(), 2);
        plan.assert_async().await;
        limits.assert_async().await;
    }

    #[tokio::test]
    async fn a_lapsed_token_serves_the_cached_answer_aged_to_now() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/users/me/plan")
            .with_body(PLAN_BODY)
            .expect(1)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/users/me/plan/usage-limits")
            .with_body(LIMITS_BODY)
            .expect(1)
            .create_async()
            .await;
        let (_dir, cache) = cache_fixture();
        let endpoints = Endpoints { base: server.url() };
        refresh(&cache, &endpoints, &credential(), Duration::ZERO, now())
            .await
            .expect("a refresh while the token was live");

        let lapsed = Credential {
            expired: true,
            ..credential()
        };
        let output = refresh(
            &cache,
            &endpoints,
            &lapsed,
            Duration::ZERO,
            now() + chrono::Duration::days(2),
        )
        .await
        .expect("the cached answer is still worth showing");

        assert!(output.stale);
        assert_eq!(output.snapshot.seven_day.expect("weekly").pct(), 0);
        assert_eq!(output.snapshot.thirty_day.expect("monthly").pct(), 2);
        let (code, message) = output.last_error.expect("a recorded reason");
        assert_eq!(code, 401);
        assert!(message.contains("expired"));
    }

    #[tokio::test]
    async fn a_lapsed_token_with_no_cache_reports_the_sign_in_instead() {
        let (_dir, cache) = cache_fixture();
        let lapsed = Credential {
            expired: true,
            ..credential()
        };

        let error = refresh(
            &cache,
            &Endpoints {
                base: "http://127.0.0.1:1".to_string(),
            },
            &lapsed,
            Duration::ZERO,
            now(),
        )
        .await
        .expect_err("nothing to show and nothing to fetch");
        assert!(error.to_string().contains("expired"));
    }

    #[tokio::test]
    async fn unauthorized_without_cache_returns_a_redacted_error() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/users/me/plan")
            .with_status(401)
            .with_body("workos:test-token should never be echoed")
            .create_async()
            .await;
        let (_dir, cache) = cache_fixture();

        let error = refresh(
            &cache,
            &Endpoints { base: server.url() },
            &credential(),
            Duration::ZERO,
            now(),
        )
        .await
        .expect_err("401 without a cache must fail");

        let rendered = error.to_string();
        assert!(rendered.contains("401"));
        assert!(!rendered.contains("test-token"));
    }

    #[tokio::test]
    async fn a_malformed_limits_response_is_a_schema_error_not_zero_percent() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/users/me/plan")
            .with_body(PLAN_BODY)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/users/me/plan/usage-limits")
            .with_body(r#"{"data":{"limits":[{"type":"weekly"}]},"success":true}"#)
            .create_async()
            .await;
        let (_dir, cache) = cache_fixture();

        let error = refresh(
            &cache,
            &Endpoints { base: server.url() },
            &credential(),
            Duration::ZERO,
            now(),
        )
        .await
        .expect_err("a limit without a percentage must not render");
        assert!(matches!(error, AppError::Schema(_)));
    }

    #[tokio::test]
    async fn a_cache_written_for_another_account_is_not_served() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/users/me/plan")
            .with_body(PLAN_BODY)
            .expect(2)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v1/users/me/plan/usage-limits")
            .with_body(LIMITS_BODY)
            .expect(1)
            .create_async()
            .await;
        // The second refresh (another account, so no usable cache) hits the
        // same routes and fails on the limits call.
        server
            .mock("GET", "/api/v1/users/me/plan/usage-limits")
            .with_status(503)
            .create_async()
            .await;
        let (_dir, cache) = cache_fixture();
        let endpoints = Endpoints { base: server.url() };
        refresh(&cache, &endpoints, &credential(), Duration::ZERO, now())
            .await
            .expect("first account refresh");

        let other = Credential {
            account_id: Some("usr-2".to_string()),
            ..credential()
        };
        let error = refresh(&cache, &endpoints, &other, Duration::ZERO, now())
            .await
            .expect_err("another account's cache must not be served");
        assert!(matches!(error, AppError::Http { status: 503, .. }));
    }

    #[test]
    fn a_ledger_cache_from_the_previous_schema_is_ignored() {
        let endpoints = Endpoints::default();
        let old = format!(
            r#"{{"target":"{}|user:x|v2","plan":{{}},"ledger":[]}}"#,
            endpoints.base
        );
        assert!(parse_cache(old.as_bytes(), &target_key(&endpoints, "usr-1")).is_err());
    }
}
