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

/// Nominal window lengths, for pacing. The endpoint reports only the reset
/// instant, so the cadence each window is named for is the length: a "week"
/// window that resets on Sunday is seven days long.
const TODAY_LEN: chrono::TimeDelta = chrono::TimeDelta::days(1);
const WEEK_LEN: chrono::TimeDelta = chrono::TimeDelta::days(7);
const MONTH_LEN: chrono::TimeDelta = chrono::TimeDelta::days(30);

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
    fn into_window(self, window_duration: chrono::Duration) -> ShviaWindow {
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
            window_duration,
        }
    }
}

impl Envelope {
    /// Project the envelope into the canonical [`ShviaSnapshot`]. The
    /// `config_plan` is an optional display-only label for the tooltip header
    /// (defaults to `"ShvIA"`).
    pub fn into_snapshot(self, config_plan: Option<&str>) -> ShviaSnapshot {
        ShviaSnapshot {
            plan: plan_label(config_plan),
            today: self.today.map(|w| w.into_window(TODAY_LEN)),
            week: self.week.map(|w| w.into_window(WEEK_LEN)),
            month: self.month.map(|w| w.into_window(MONTH_LEN)),
        }
    }
}

/// The tooltip header's label: the configured one when it says something, the
/// vendor's own name otherwise.
///
/// It is a single header line, so it goes through
/// [`sanitize_untrusted_line`](crate::display::sanitize_untrusted_line) rather
/// than the field sanitizer, which keeps newlines by design — one embedded
/// newline in a config-supplied label would otherwise forge a tooltip row.
pub fn plan_label(config_plan: Option<&str>) -> String {
    let raw = config_plan
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("ShvIA");
    crate::display::sanitize_untrusted_line(raw)
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
        assert_eq!(today.window_duration, TODAY_LEN);

        let week = snap.week.as_ref().unwrap();
        assert_eq!(week.used, 9000);
        assert_eq!(week.remaining, Some(491000));
        assert_eq!(week.window_duration, WEEK_LEN);

        // month has limit -1 → unlimited, remaining forced to None.
        let month = snap.month.as_ref().unwrap();
        assert!(month.is_unlimited());
        assert_eq!(month.utilization_pct(), 0);
        assert_eq!(month.remaining, None);
        // No ratio, so nothing for the shared bar renderer to draw.
        assert!(month.as_usage_window().is_none());
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
        let snap = env.into_snapshot(Some("   "));
        assert_eq!(snap.plan, "ShvIA");
    }

    /// The label is config-supplied and lands in Pango markup, so a newline or
    /// a control character in it must not reach the tooltip intact.
    #[test]
    fn plan_label_is_sanitized() {
        let snap = Envelope::default().into_snapshot(Some("Gate\nway"));
        assert!(!snap.plan.contains('\n'), "got {:?}", snap.plan);
    }
}
