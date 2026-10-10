//! Fetch Cursor's included-usage summary from `GET /api/usage-summary`,
//! authenticated with the session token read out of the local `state.vscdb`
//! (see `db.rs`). Cache/stale/error-fallback shape mirrors `kimi::fetch`.
//!
//! Credit grants are a second call, `POST GetClientVisibleCreditGrants` on
//! `api2.cursor.sh`, with the same access token as a Bearer credential. That
//! call is best-effort: a timeout, a non-2xx, or a body that is not a grant
//! list leaves `credits` empty and the usage bars up.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::cache::{Cache, acquire_lock_async};
use crate::error::{AppError, Result};
use crate::usage::{CursorCreditGrant, CursorSnapshot};
use crate::vendor::{MAX_BODY_BYTES, read_body_capped};

use super::db;
use super::types::{self, UsageSummary};

pub const BASE_URL: &str = "https://cursor.com";
/// Connect-RPC the spending page's Credits card is drawn from. Same access
/// token as the usage-summary cookie, sent as a Bearer credential.
pub const CREDITS_URL: &str =
    "https://api2.cursor.sh/aiserver.v1.DashboardService/GetClientVisibleCreditGrants";
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_TIMEOUT: Duration = Duration::from_secs(15);
/// The dashboard endpoint gates on browser-looking headers; a plain
/// `reqwest` request with only the cookie is rejected by its CORS check.
const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub summary: String,
    pub credits: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            summary: format!("{BASE_URL}/api/usage-summary"),
            credits: CREDITS_URL.into(),
        }
    }
}

/// This vendor's [`Outcome`](crate::outcome::Outcome) — the shared shape,
/// specialised to its snapshot.
pub type FetchOutcome = crate::outcome::Outcome<CursorSnapshot>;

/// Cache-aware fetch. `db_path` is Cursor's `state.vscdb` — the caller resolves
/// `[cursor] db_path` (config override) vs [`db::default_db_path`], the same
/// override pattern as `openai.codex_auth_path`. `agent_auth_path` is the
/// headless `cursor-agent` CLI's own `auth.json`, tried when `db_path` is
/// missing — see `db::resolve_access_token`, which on macOS also tries the
/// CLI's Keychain items.
pub async fn fetch_snapshot(
    client: &reqwest::Client,
    db_path: &Path,
    agent_auth_path: &Path,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
) -> Result<FetchOutcome> {
    fetch_snapshot_at(
        client,
        db_path,
        agent_auth_path,
        cache,
        endpoints,
        cache_ttl,
        Utc::now(),
    )
    .await
}

/// Clock seam for cache rollover tests. Cursor's payload describes one billing
/// cycle, so serving it after `reset_at` would knowingly show the prior cycle.
async fn fetch_snapshot_at(
    client: &reqwest::Client,
    db_path: &Path,
    agent_auth_path: &Path,
    cache: &Cache,
    endpoints: &Endpoints,
    cache_ttl: Duration,
    now: DateTime<Utc>,
) -> Result<FetchOutcome> {
    cache.ensure_dir()?;
    let _lock = acquire_lock_async(&cache.lock_path(), LOCK_TIMEOUT).await?;

    // Resolve the local identity before accepting a cache hit. Cursor can switch
    // accounts in-place in this database; returning the cache first would show
    // the previous account's private usage until the TTL elapsed.
    let token = db::resolve_access_token(db_path, agent_auth_path)?;
    let auth = db::session_auth(&token)?;

    // A payload written before credit grants existed has no `credits` key.
    // Serving it fresh would hide a balance the spending page is showing
    // until the TTL elapsed, so that cache is not a hit.
    if let Some(bytes) = cache.fresh_payload(cache_ttl)?
        && payload_records_credits(&bytes)
        && let Ok(outcome) = reuse_cache(&bytes, cache, false, &auth.account_key, now)
    {
        return Ok(outcome);
    }

    match fetch_live(client, endpoints, &auth, &token).await {
        Ok(snap) => {
            let bytes = serde_json::to_vec(&snap_to_json(&snap, &auth.account_key))?;
            cache.write_payload(&bytes)?;
            Ok(crate::outcome::Outcome::fresh(snap))
        }
        Err(e) if e.is_transient() => fallback_silent(cache, &auth.account_key, now, e),
        Err(e) => {
            cache.mark_stale();
            if let Some((code, msg)) = error_to_pair(&e) {
                cache.write_last_error(code, &msg);
            }
            fallback_with_error(cache, &auth.account_key, now, e)
        }
    }
}

fn fallback_silent(
    cache: &Cache,
    account: &str,
    now: DateTime<Utc>,
    original: AppError,
) -> Result<FetchOutcome> {
    crate::outcome::fallback(cache, None, original, |bytes| {
        parse_cache_at(bytes, account, now)
    })
}

fn fallback_with_error(
    cache: &Cache,
    account: &str,
    now: DateTime<Utc>,
    original: AppError,
) -> Result<FetchOutcome> {
    let last_error = error_to_pair(&original);
    crate::outcome::fallback(cache, last_error, original, |bytes| {
        parse_cache_at(bytes, account, now)
    })
}

/// Never surface upstream bodies for auth failures: the request carried a
/// session cookie derived from a signed-in token, and 401/403 bodies from a
/// scraped web endpoint are not guaranteed not to echo it back.
fn error_to_pair(e: &AppError) -> Option<(u16, String)> {
    match e {
        AppError::Http { status, .. } if matches!(status, 401 | 403) => {
            Some((*status, "Cursor authentication failed".into()))
        }
        AppError::Http { status, body } => Some((*status, body.clone())),
        e => Some((0, e.to_string())),
    }
}

fn reuse_cache(
    bytes: &[u8],
    cache: &Cache,
    stale: bool,
    account: &str,
    now: DateTime<Utc>,
) -> Result<FetchOutcome> {
    let snap = parse_cache_at(bytes, account, now)?;
    Ok(crate::outcome::Outcome::cached(snap, cache, stale))
}

fn parse_cache_at(bytes: &[u8], account: &str, now: DateTime<Utc>) -> Result<CursorSnapshot> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    if v.get("account").and_then(serde_json::Value::as_str) != Some(account) {
        return Err(AppError::Schema(
            "cursor cache belongs to a different account; refetching".into(),
        ));
    }
    let int = |key: &str| -> Result<i32> {
        v[key]
            .as_i64()
            .filter(|n| *n >= 0)
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| AppError::Schema(format!("cursor cache: invalid {key}")))
    };
    let optional_cents = |key: &str| -> Result<Option<i64>> {
        match v.get(key) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(value) => value
                .as_i64()
                .filter(|n| *n >= 0)
                .map(Some)
                .ok_or_else(|| AppError::Schema(format!("cursor cache: invalid {key}"))),
        }
    };
    let plan = v["plan"]
        .as_str()
        .filter(|plan| !plan.trim().is_empty())
        .ok_or_else(|| AppError::Schema("cursor cache: invalid plan".into()))?
        .to_string();
    let reset_at = parse_cache_datetime(&v["reset_at"])?
        .ok_or_else(|| AppError::Schema("cursor cache: missing reset timestamp".into()))?;
    if reset_at <= now {
        return Err(AppError::Schema(
            "cursor cache is past its billing-cycle reset; refetching".into(),
        ));
    }
    Ok(CursorSnapshot {
        plan,
        auto_pct: int("auto_pct")?,
        api_pct: int("api_pct")?,
        total_pct: int("total_pct")?,
        unlimited: v["unlimited"]
            .as_bool()
            .ok_or_else(|| AppError::Schema("cursor cache: invalid unlimited flag".into()))?,
        on_demand_enabled: v["on_demand_enabled"]
            .as_bool()
            .ok_or_else(|| AppError::Schema("cursor cache: invalid on-demand flag".into()))?,
        on_demand_used_cents: optional_cents("on_demand_used_cents")?,
        on_demand_limit_cents: optional_cents("on_demand_limit_cents")?,
        reset_at: Some(reset_at),
        cycle_start: v
            .get("cycle_start")
            .and_then(|c| parse_cache_datetime(c).ok())
            .flatten(),
        credits: parse_cached_credits(&v),
    })
}

/// A missing `credits` key is a cache written before grants existed. A bad
/// entry is skipped so one corrupt grant cannot throw away the usage bars.
/// `credits` must be present, even as an empty list. Absence means the
/// payload predates the field and is not a complete snapshot.
fn payload_records_credits(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .is_some_and(|value| value.get("credits").is_some())
}

fn parse_cached_credits(v: &serde_json::Value) -> Vec<CursorCreditGrant> {
    let Some(items) = v.get("credits").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    items.iter().filter_map(cached_credit).collect()
}

fn cached_credit(item: &serde_json::Value) -> Option<CursorCreditGrant> {
    let remaining_cents = item.get("remaining_cents")?.as_i64().filter(|n| *n >= 0)?;
    let total_cents = item.get("total_cents")?.as_i64().filter(|n| *n > 0)?;
    let expires_at = match item.get("expires_at") {
        None | Some(serde_json::Value::Null) => None,
        Some(raw) => Some(parse_cache_datetime(raw).ok().flatten()?),
    };
    let display_name = item
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(160)
        .collect();
    Some(CursorCreditGrant {
        remaining_cents,
        total_cents,
        expires_at,
        display_name,
    })
}

fn parse_cache_datetime(v: &serde_json::Value) -> Result<Option<DateTime<Utc>>> {
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => DateTime::parse_from_rfc3339(s)
            .map(|dt| Some(dt.into()))
            .map_err(|e| AppError::Schema(format!("cursor cache: invalid reset timestamp: {e}"))),
        _ => Err(AppError::Schema(
            "cursor cache: invalid reset timestamp".into(),
        )),
    }
}

fn snap_to_json(snap: &CursorSnapshot, account: &str) -> serde_json::Value {
    serde_json::json!({
        "account": account,
        "plan": snap.plan,
        "auto_pct": snap.auto_pct,
        "api_pct": snap.api_pct,
        "total_pct": snap.total_pct,
        "unlimited": snap.unlimited,
        "on_demand_enabled": snap.on_demand_enabled,
        "on_demand_used_cents": snap.on_demand_used_cents,
        "on_demand_limit_cents": snap.on_demand_limit_cents,
        "reset_at": snap.reset_at.map(|dt| dt.to_rfc3339()),
        "cycle_start": snap.cycle_start.map(|dt| dt.to_rfc3339()),
        "credits": snap.credits.iter().map(|grant| serde_json::json!({
            "remaining_cents": grant.remaining_cents,
            "total_cents": grant.total_cents,
            "expires_at": grant.expires_at.map(|dt| dt.to_rfc3339()),
            "display_name": grant.display_name,
        })).collect::<Vec<_>>(),
    })
}

async fn fetch_live(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    auth: &db::SessionAuth,
    access_token: &str,
) -> Result<CursorSnapshot> {
    // usage-summary keys off the session cookie alone (no `?user=` param); the
    // browser-ish headers get past its CORS gate.
    let resp = tokio::time::timeout(
        HTTP_TIMEOUT,
        client
            .get(&endpoints.summary)
            .header(
                "Cookie",
                format!("WorkosCursorSessionToken={}", auth.cookie_value),
            )
            .header("Origin", BASE_URL)
            .header("Referer", format!("{BASE_URL}/dashboard"))
            .header("User-Agent", BROWSER_UA)
            .send(),
    )
    .await
    .map_err(|_| AppError::Transport(format!("cursor timeout: {}", endpoints.summary)))??;

    let status = resp.status();
    if !status.is_success() {
        let body = if matches!(status.as_u16(), 401 | 403) {
            "Cursor authentication failed".into()
        } else {
            format!("Cursor API returned HTTP {}", status.as_u16())
        };
        return Err(AppError::Http {
            status: status.as_u16(),
            body,
        });
    }

    let bytes = read_body_capped(resp, MAX_BODY_BYTES).await?;
    let parsed: UsageSummary = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Schema(format!("cursor usage-summary response: {e}")))?;
    let mut snap = types::to_snapshot(parsed)?;
    snap.credits = fetch_credits(client, endpoints, access_token).await;
    Ok(snap)
}

/// Best-effort. Anything other than a 2xx grant list — including auth failure
/// on this call alone — is an empty balance, not an error on the usage bars.
async fn fetch_credits(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    access_token: &str,
) -> Vec<CursorCreditGrant> {
    let Ok(Ok(resp)) = tokio::time::timeout(
        HTTP_TIMEOUT,
        client
            .post(&endpoints.credits)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Connect-Protocol-Version", "1")
            .body("{}")
            .send(),
    )
    .await
    else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let Ok(bytes) = read_body_capped(resp, MAX_BODY_BYTES).await else {
        return Vec::new();
    };
    types::parse_credit_grants(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn cache_fixture() -> (TempDir, Cache) {
        let td = TempDir::new().unwrap();
        let cache = Cache::at(td.path().join("cursor"));
        cache.ensure_dir().unwrap();
        (td, cache)
    }

    /// A minimal, unsigned JWT with `sub: "auth0|<user_id>"` — signature
    /// verification is never performed (see `db::parse_jwt_claims`).
    fn fake_token(user_id: &str) -> String {
        use base64::Engine;
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::json!({"sub": format!("auth0|{user_id}")}).to_string());
        format!("{header}.{payload}.sig")
    }

    fn seed_state_db(dir: &TempDir, token: &str) -> std::path::PathBuf {
        let path = dir.path().join("state.vscdb");
        let conn = Connection::open(&path).unwrap();
        conn.execute("CREATE TABLE ItemTable (key TEXT, value TEXT)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES ('cursorAuth/accessToken', ?1)",
            [token],
        )
        .unwrap();
        path
    }

    fn account_key(token: &str) -> String {
        db::session_auth(token).unwrap().account_key
    }

    /// A path that never exists, for tests that only care about the IDE
    /// `db_path` and want the agent fallback to stay out of the way.
    fn no_agent_auth() -> std::path::PathBuf {
        std::path::PathBuf::from("/nonexistent/cursor-agent-auth.json")
    }

    fn endpoints_for(server: &mockito::Server) -> Endpoints {
        Endpoints {
            summary: format!("{}/api/usage-summary", server.url()),
            credits: format!(
                "{}/aiserver.v1.DashboardService/GetClientVisibleCreditGrants",
                server.url()
            ),
        }
    }

    fn cached_snapshot(account: &str, reset_at: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "account": account,
            "plan": "Ultra",
            "auto_pct": 40,
            "api_pct": 10,
            "total_pct": 30,
            "unlimited": false,
            "on_demand_enabled": false,
            "reset_at": reset_at,
        }))
        .unwrap()
    }

    fn sample_json() -> String {
        r#"{
            "billingCycleEnd": "2099-08-04T00:35:51.000Z",
            "membershipType": "ultra",
            "isUnlimited": false,
            "individualUsage": {
                "plan": { "autoPercentUsed": 98.109, "apiPercentUsed": 100, "totalPercentUsed": 98.5 },
                "onDemand": { "enabled": false }
            }
        }"#
        .to_string()
    }

    #[tokio::test]
    async fn live_fetch_reads_token_from_db_and_sends_the_session_cookie() {
        let mut server = mockito::Server::new_async().await;
        let token = fake_token("user_123");
        let m = server
            .mock("GET", "/api/usage-summary")
            .match_header(
                "cookie",
                format!("WorkosCursorSessionToken=user_123%3A%3A{token}").as_str(),
            )
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;

        let db_dir = TempDir::new().unwrap();
        let db_path = seed_state_db(&db_dir, &token);
        let (_cache_dir, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = endpoints_for(&server);

        let out = fetch_snapshot(
            &client,
            &db_path,
            &no_agent_auth(),
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        m.assert_async().await;
        assert_eq!(out.snapshot.plan, "Ultra");
        assert_eq!(out.snapshot.auto_pct, 98);
        assert_eq!(out.snapshot.api_pct, 100);
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn missing_db_file_is_a_credentials_error_with_no_cache_to_fall_back_on() {
        let (_cache_dir, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = Endpoints::default();
        let db_path = std::path::Path::new("/nonexistent/state.vscdb");

        let err = fetch_snapshot(
            &client,
            db_path,
            &no_agent_auth(),
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Credentials(_)));
    }

    #[tokio::test]
    async fn agent_auth_file_is_used_when_the_ide_db_is_missing() {
        let mut server = mockito::Server::new_async().await;
        let token = fake_token("user_123");
        let m = server
            .mock("GET", "/api/usage-summary")
            .match_header(
                "cookie",
                format!("WorkosCursorSessionToken=user_123%3A%3A{token}").as_str(),
            )
            .with_status(200)
            .with_body(sample_json())
            .create_async()
            .await;

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("state.vscdb"); // deliberately never seeded
        let agent_path = dir.path().join("auth.json");
        std::fs::write(
            &agent_path,
            serde_json::json!({"accessToken": token, "refreshToken": "r"}).to_string(),
        )
        .unwrap();
        let (_cache_dir, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = endpoints_for(&server);

        let out = fetch_snapshot(
            &client,
            &db_path,
            &agent_path,
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        m.assert_async().await;
        assert_eq!(out.snapshot.plan, "Ultra");
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn http_error_falls_back_to_cache_and_hides_the_upstream_body() {
        let mut server = mockito::Server::new_async().await;
        let token = fake_token("user_123");
        server
            .mock("GET", "/api/usage-summary")
            .with_status(401)
            .with_body(r#"{"detail":"leaked-looking body"}"#)
            .create_async()
            .await;

        let db_dir = TempDir::new().unwrap();
        let db_path = seed_state_db(&db_dir, &token);
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(&cached_snapshot(
                &account_key(&token),
                "2099-08-04T00:00:00Z",
            ))
            .unwrap();

        let client = reqwest::Client::new();
        let endpoints = endpoints_for(&server);
        let out = fetch_snapshot(
            &client,
            &db_path,
            &no_agent_auth(),
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        assert!(out.stale);
        assert_eq!(out.snapshot.auto_pct, 40);
        let (code, msg) = out.last_error.unwrap();
        assert_eq!(code, 401);
        assert_eq!(msg, "Cursor authentication failed");
        assert!(!msg.contains("leaked-looking"));
    }

    #[tokio::test]
    async fn fresh_cache_is_used_after_verifying_the_current_account() {
        let token = fake_token("user_123");
        let db_dir = TempDir::new().unwrap();
        let db_path = seed_state_db(&db_dir, &token);
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(
                serde_json::json!({
                    "account": account_key(&token),
                    "plan": "Pro", "auto_pct": 7, "api_pct": 3, "total_pct": 5,
                    "unlimited": false, "on_demand_enabled": true,
                    "on_demand_used_cents": 1785,
                    "on_demand_limit_cents": 35000,
                    "reset_at": "2099-08-04T00:00:00Z",
                    "credits": [],
                })
                .to_string()
                .as_bytes(),
            )
            .unwrap();

        let client = reqwest::Client::new();
        let endpoints = Endpoints::default();
        let out = fetch_snapshot(
            &client,
            &db_path,
            &no_agent_auth(),
            &cache,
            &endpoints,
            Duration::from_secs(3600),
        )
        .await
        .unwrap();
        assert_eq!(out.snapshot.auto_pct, 7);
        assert!(out.snapshot.on_demand_enabled);
        assert_eq!(out.snapshot.on_demand_used_cents, Some(1785));
        assert_eq!(out.snapshot.on_demand_limit_cents, Some(35000));
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn fresh_cache_without_a_credits_field_is_refetched() {
        let token = fake_token("user_123");
        let db_dir = TempDir::new().unwrap();
        let db_path = seed_state_db(&db_dir, &token);
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(
                serde_json::json!({
                    "account": account_key(&token),
                    "plan": "Pro", "auto_pct": 7, "api_pct": 3, "total_pct": 5,
                    "unlimited": false, "on_demand_enabled": false,
                    "reset_at": "2099-08-04T00:00:00Z",
                })
                .to_string()
                .as_bytes(),
            )
            .unwrap();

        let mut server = mockito::Server::new_async().await;
        let summary = server
            .mock("GET", "/api/usage-summary")
            .with_status(200)
            .with_body(sample_json())
            .expect(1)
            .create_async()
            .await;
        let grants = server
            .mock(
                "POST",
                "/aiserver.v1.DashboardService/GetClientVisibleCreditGrants",
            )
            .with_status(200)
            .with_body(
                r#"{"grants":[{"remainingCents":2100,"totalCents":2500,"expiresAtMs":1793577600000,"displayName":"Promo"}]}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let out = fetch_snapshot(
            &reqwest::Client::new(),
            &db_path,
            &no_agent_auth(),
            &cache,
            &endpoints_for(&server),
            Duration::from_secs(3600),
        )
        .await
        .unwrap();
        summary.assert_async().await;
        grants.assert_async().await;
        assert_eq!(out.snapshot.auto_pct, 98);
        assert_eq!(out.snapshot.credits.len(), 1);
        assert_eq!(out.snapshot.credits[0].remaining_cents, 2100);
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn switching_accounts_rejects_a_fresh_cache_and_refetches() {
        let old_token = fake_token("old_account");
        let new_token = fake_token("new_account");
        let db_dir = TempDir::new().unwrap();
        let db_path = seed_state_db(&db_dir, &new_token);
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(&cached_snapshot(
                &account_key(&old_token),
                "2099-08-04T00:00:00Z",
            ))
            .unwrap();

        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/api/usage-summary")
            .match_header(
                "cookie",
                format!("WorkosCursorSessionToken=new_account%3A%3A{new_token}").as_str(),
            )
            .with_status(200)
            .with_body(sample_json())
            .expect(1)
            .create_async()
            .await;
        let endpoints = endpoints_for(&server);

        let out = fetch_snapshot(
            &reqwest::Client::new(),
            &db_path,
            &no_agent_auth(),
            &cache,
            &endpoints,
            Duration::from_secs(3600),
        )
        .await
        .unwrap();
        request.assert_async().await;
        assert_eq!(out.snapshot.auto_pct, 98);
        assert!(!out.stale);
    }

    #[tokio::test]
    async fn cache_past_its_billing_reset_is_not_served_during_an_outage() {
        let token = fake_token("user_123");
        let db_dir = TempDir::new().unwrap();
        let db_path = seed_state_db(&db_dir, &token);
        let (_cache_dir, cache) = cache_fixture();
        cache
            .write_payload(&cached_snapshot(
                &account_key(&token),
                "2026-08-04T00:00:00Z",
            ))
            .unwrap();

        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/usage-summary")
            .with_status(503)
            .create_async()
            .await;
        let endpoints = endpoints_for(&server);
        let now = DateTime::parse_from_rfc3339("2026-08-05T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let err = fetch_snapshot_at(
            &reqwest::Client::new(),
            &db_path,
            &no_agent_auth(),
            &cache,
            &endpoints,
            Duration::from_secs(0),
            now,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Http { status: 503, .. }));
    }

    #[test]
    fn cached_percentages_are_range_checked_before_narrowing() {
        let now = DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut payload: serde_json::Value =
            serde_json::from_slice(&cached_snapshot("account", "2026-08-04T00:00:00Z")).unwrap();
        payload["auto_pct"] = serde_json::json!(i64::MAX);
        let err =
            parse_cache_at(&serde_json::to_vec(&payload).unwrap(), "account", now).unwrap_err();
        assert!(matches!(err, AppError::Schema(_)));
    }

    #[tokio::test]
    async fn credit_grants_ride_along_and_a_failed_grant_call_keeps_the_bars() {
        let mut server = mockito::Server::new_async().await;
        let token = fake_token("user_123");
        let summary = server
            .mock("GET", "/api/usage-summary")
            .with_status(200)
            .with_body(sample_json())
            .expect(2)
            .create_async()
            .await;
        let grants = server
            .mock(
                "POST",
                "/aiserver.v1.DashboardService/GetClientVisibleCreditGrants",
            )
            .match_header("authorization", format!("Bearer {token}").as_str())
            .match_header("connect-protocol-version", "1")
            .match_body("{}")
            .with_status(200)
            .with_body(
                r#"{"grants":[{"remainingCents":"2100","totalCents":2500,"expiresAtMs":"1793577600000","displayName":"Promo"}]}"#,
            )
            .expect(1)
            .create_async()
            .await;

        let db_dir = TempDir::new().unwrap();
        let db_path = seed_state_db(&db_dir, &token);
        let (_cache_dir, cache) = cache_fixture();
        let client = reqwest::Client::new();
        let endpoints = endpoints_for(&server);

        let out = fetch_snapshot(
            &client,
            &db_path,
            &no_agent_auth(),
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        grants.assert_async().await;
        assert!(!out.stale);
        assert_eq!(out.snapshot.auto_pct, 98);
        assert_eq!(out.snapshot.credits.len(), 1);
        assert_eq!(out.snapshot.credits[0].remaining_cents, 2100);
        assert_eq!(out.snapshot.credits[0].total_cents, 2500);
        assert_eq!(out.snapshot.credits[0].display_name, "Promo");
        assert!(out.last_error.is_none());

        // Registered after the success mock so it is the one the refetch hits.
        // A grant-call failure must not stale the bars or surface the body.
        let down = server
            .mock(
                "POST",
                "/aiserver.v1.DashboardService/GetClientVisibleCreditGrants",
            )
            .with_status(503)
            .with_body("upstream body")
            .expect(1)
            .create_async()
            .await;
        let again = fetch_snapshot(
            &client,
            &db_path,
            &no_agent_auth(),
            &cache,
            &endpoints,
            Duration::from_secs(0),
        )
        .await
        .unwrap();
        down.assert_async().await;
        summary.assert_async().await;
        assert!(!again.stale);
        assert_eq!(again.snapshot.auto_pct, 98);
        assert!(again.snapshot.credits.is_empty());
        assert!(again.last_error.is_none());
    }

    #[test]
    fn cache_round_trips_credit_grants_and_skips_a_bad_one() {
        let now = DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expires = DateTime::parse_from_rfc3339("2026-11-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let snap = CursorSnapshot {
            plan: "Pro".into(),
            auto_pct: 1,
            api_pct: 2,
            total_pct: 1,
            unlimited: false,
            on_demand_enabled: false,
            on_demand_used_cents: None,
            on_demand_limit_cents: None,
            reset_at: Some(
                DateTime::parse_from_rfc3339("2026-08-04T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            cycle_start: None,
            credits: vec![CursorCreditGrant {
                remaining_cents: 2100,
                total_cents: 2500,
                expires_at: Some(expires),
                display_name: "Promo".into(),
            }],
        };
        let parsed = parse_cache_at(
            &serde_json::to_vec(&snap_to_json(&snap, "account")).unwrap(),
            "account",
            now,
        )
        .unwrap();
        assert_eq!(parsed.credits, snap.credits);

        let mut legacy: serde_json::Value =
            serde_json::from_slice(&cached_snapshot("account", "2026-08-04T00:00:00Z")).unwrap();
        let without =
            parse_cache_at(&serde_json::to_vec(&legacy).unwrap(), "account", now).unwrap();
        assert!(without.credits.is_empty());

        legacy["credits"] = serde_json::json!([
            {"remaining_cents": -1, "total_cents": 100},
            {"remaining_cents": 2100, "total_cents": 2500, "display_name": "Kept"}
        ]);
        let mixed = parse_cache_at(&serde_json::to_vec(&legacy).unwrap(), "account", now).unwrap();
        assert_eq!(mixed.credits.len(), 1);
        assert_eq!(mixed.credits[0].display_name, "Kept");
        assert!(mixed.credits[0].expires_at.is_none());
    }
}
