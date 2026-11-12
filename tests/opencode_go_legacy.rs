//! Compatibility coverage from the v1 credential store through quota rendering.
//! Every path, HTTP endpoint, theme and timestamp is injected.

use std::time::Duration;

use ai_usagebar::cache::Cache;
use ai_usagebar::opencode_go::{creds, fetch, vendor};
use ai_usagebar::theme::Theme;
use ai_usagebar::vendor::{RenderOpts, VendorOutcome};
use serde_json::json;

#[tokio::test]
async fn v1_credentials_keep_working_without_v2_and_with_valid_or_corrupt_v2_stores() {
    let mut server = mockito::Server::new_async().await;
    let client = reqwest::Client::new();
    let endpoints = fetch::Endpoints {
        usage: format!("{}/zen/go/v1/usage", server.url()),
    };
    let now = "2026-08-16T18:00:00Z".parse().unwrap();
    let opts = RenderOpts {
        format: None,
        tooltip_format: None,
        icon: None,
        pace_tolerance: 5,
        format_pace_color: false,
        tooltip_pace_pts: false,
    };

    for kind in ["api", "zen"] {
        for database_state in ["missing", "valid", "corrupt"] {
            let dir = tempfile::tempdir().unwrap();
            let legacy = dir.path().join("auth.json");
            let database = dir.path().join("opencode.db");
            let contents = serde_json::to_vec(&json!({
                "opencode": {"type":kind, "key":"  legacy-test-key  ", "metadata":{"future":"ignored"}},
                "another-provider": {"type":"api", "key":"other-test-key"}
            })).unwrap();
            std::fs::write(&legacy, &contents).unwrap();
            match database_state {
                "valid" => {
                    let conn = rusqlite::Connection::open(&database).unwrap();
                    conn.execute_batch("CREATE TABLE credential (integration_id TEXT, active INTEGER, time_updated INTEGER, value TEXT);").unwrap();
                    conn.execute(
                        "INSERT INTO credential VALUES ('opencode', 1, 2, ?1)",
                        [json!({"type":"key", "key":"different-v2-account"}).to_string()],
                    )
                    .unwrap();
                }
                "corrupt" => std::fs::write(&database, b"invalid database").unwrap(),
                _ => {}
            }
            let database_before = std::fs::read(&database).ok();
            let paths = [legacy.clone(), database.clone()];
            assert!(creds::probe(Some(&paths)));
            let key = creds::resolve_from("OPENCODE_GO_API_KEY", None, None, &paths).unwrap();
            assert_eq!(
                key, "legacy-test-key",
                "v1 {kind}, database {database_state}"
            );
            let mock = server
                .mock("GET", "/zen/go/v1/usage")
                .match_header("authorization", "Bearer legacy-test-key")
                .match_header("accept", "application/json")
                .with_status(200)
                .with_body(include_str!("fixtures/opencode-go/usage.json"))
                .create_async()
                .await;
            let outcome = fetch::fetch_snapshot(
                &client,
                &key,
                &Cache::at(dir.path().join("cache")),
                &endpoints,
                Duration::from_secs(60),
            )
            .await
            .unwrap();
            assert_eq!(outcome.snapshot.rolling.as_ref().unwrap().percent, 12.3);
            assert_eq!(outcome.snapshot.weekly.as_ref().unwrap().percent, 45.6);
            assert_eq!(outcome.snapshot.monthly.as_ref().unwrap().percent, 78.9);
            let shared: VendorOutcome = outcome.clone().into();
            let output = vendor::render(&shared, &outcome.snapshot, &Theme::default(), &opts, now);
            assert!(output.text.contains("12.3%"));
            for label in ["Rolling (5h)", "Weekly (7d)", "Monthly"] {
                assert!(output.tooltip.contains(label));
            }
            assert!(!output.text.contains(&key) && !output.tooltip.contains(&key));
            assert_eq!(std::fs::read(&legacy).unwrap(), contents);
            assert_eq!(std::fs::read(&database).ok(), database_before);
            mock.assert_async().await;
        }
    }
}
