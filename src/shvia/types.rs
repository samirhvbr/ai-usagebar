//! Wire types for the ShvIA gateway usage endpoint
//! `{base_url}/api/v1/usage`.
//!
//! Response shape:
//!
//! ```json
//! {
//!   "today": {"used": 1234, "limit": 100000, "remaining": 98766,
//!             "reset_at": "2026-06-24T00:00:00Z"},
//!   "week":  {"used": 9000, "limit": 500000, "remaining": 491000,
//!             "reset_at": "2026-06-29T00:00:00Z"},
//!   "month": {"used": 40000, "limit": -1, "remaining": null,
//!             "reset_at": "2026-07-01T00:00:00Z"}
//! }
//! ```
//!
//! Semantics: `limit == -1` means UNLIMITED — `remaining` is then `null` and
//! the renderer shows the raw `used` count without a percentage. When
//! `limit > 0`, utilization % = round(used / limit * 100).

use serde::Deserialize;

use crate::usage::{ShviaSnapshot, ShviaWindow};

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Envelope {
    pub today: Option<Window>,
    pub week: Option<Window>,
    pub month: Option<Window>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Window {
    pub used: i64,
    /// `-1` means unlimited.
    pub limit: i64,
    /// `null` when unlimited / unreported.
    pub remaining: Option<i64>,
    /// ISO-8601 reset timestamp; `null` / missing → `None`.
    pub reset_at: Option<String>,
}

impl Window {
    fn into_window(self) -> ShviaWindow {
        let resets_at = self
            .reset_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));
        // When the API reports an unlimited window it sends `remaining: null`;
        // keep it `None` in that case so the renderer shows the used count.
        let remaining = if self.limit <= 0 {
            None
        } else {
            self.remaining
        };
        ShviaWindow {
            used: self.used,
            limit: self.limit,
            remaining,
            resets_at,
        }
    }
}

impl Envelope {
    /// Project the envelope into the canonical [`ShviaSnapshot`]. The
    /// `config_plan` is an optional display-only label for the tooltip header
    /// (defaults to `"ShvIA"`).
    pub fn into_snapshot(self, config_plan: Option<&str>) -> ShviaSnapshot {
        let plan = config_plan
            .filter(|p| !p.is_empty())
            .unwrap_or("ShvIA")
            .to_string();
        ShviaSnapshot {
            plan,
            today: self.today.map(Window::into_window),
            week: self.week.map(Window::into_window),
            month: self.month.map(Window::into_window),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_BODY: &str = r#"{
        "today": {"used": 1234, "limit": 100000, "remaining": 98766, "reset_at": "2026-06-24T00:00:00Z"},
        "week":  {"used": 9000, "limit": 500000, "remaining": 491000, "reset_at": "2026-06-29T00:00:00Z"},
        "month": {"used": 40000, "limit": -1, "remaining": null, "reset_at": "2026-07-01T00:00:00Z"}
    }"#;

    #[test]
    fn parses_real_response_shape() {
        let env: Envelope = serde_json::from_str(REAL_BODY).unwrap();
        let snap = env.into_snapshot(None);
        assert_eq!(snap.plan, "ShvIA");

        let today = snap.today.as_ref().unwrap();
        assert_eq!(today.used, 1234);
        assert_eq!(today.utilization_pct(), 1); // 1.234% rounds to 1
        assert!(today.resets_at.is_some());

        let week = snap.week.as_ref().unwrap();
        assert_eq!(week.used, 9000);
        assert_eq!(week.remaining, Some(491000));

        // month has limit -1 → unlimited, remaining forced to None.
        let month = snap.month.as_ref().unwrap();
        assert!(month.is_unlimited());
        assert_eq!(month.utilization_pct(), 0);
        assert_eq!(month.remaining, None);
    }

    #[test]
    fn missing_windows_yield_none() {
        let env: Envelope = serde_json::from_str("{}").unwrap();
        let snap = env.into_snapshot(Some("Gateway"));
        assert_eq!(snap.plan, "Gateway");
        assert!(snap.today.is_none());
        assert!(snap.week.is_none());
        assert!(snap.month.is_none());
    }

    #[test]
    fn null_reset_at_becomes_none() {
        let body = r#"{"week":{"used":1,"limit":10,"remaining":9,"reset_at":null}}"#;
        let env: Envelope = serde_json::from_str(body).unwrap();
        let snap = env.into_snapshot(None);
        assert!(snap.week.as_ref().unwrap().resets_at.is_none());
    }

    #[test]
    fn empty_plan_falls_back_to_default_label() {
        let env: Envelope = serde_json::from_str("{}").unwrap();
        let snap = env.into_snapshot(Some(""));
        assert_eq!(snap.plan, "ShvIA");
    }
}
