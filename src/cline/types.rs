//! Cline plan and usage-limit response types.
//!
//! ClinePass has three limits — 5 hours, weekly, monthly — and Cline computes
//! them server side. `GET /api/v1/users/me/plan/usage-limits` (the call the
//! dashboard's "Usage limits" panel makes) returns, for each, the percentage
//! used and, once the window has opened, when it resets.
//!
//! These are **fixed windows**: one opens at the first charge after the
//! previous one closed and runs its full length. They are not rolling sums of
//! the last N days. An earlier version of this vendor re-derived them by
//! summing the charge ledger over a rolling 30 days; after one heavy month it
//! showed 100 % while the dashboard said 2 %. Reading the endpoint removes the
//! whole ledger walk and cannot disagree with the dashboard.
//!
//! Field names keep the old `seven_day` / `thirty_day` spelling (and the
//! `cline_7d_*` / `cline_30d_*` placeholders) for the weekly and monthly
//! limits, so existing waybar formats keep working.

use chrono::{DateTime, Utc};
use serde_json::Value;

/// `type` strings of `usage-limits`, in display order: 5 hours, weekly, monthly.
const LIMIT_TYPES: [&str; 3] = ["five_hour", "weekly", "monthly"];

/// What `/api/v1/users/me/plan` contributes: the plan's name and when the
/// current billing period ends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub name: Option<String>,
    pub period_end: Option<DateTime<Utc>>,
}

/// One limit as the server reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// Share of the limit used, as the server states it (a whole percent).
    pub percent: f64,
    /// When the window resets. `None` while no window is open (nothing has
    /// been charged since the last one closed).
    pub reset: Option<DateTime<Utc>>,
}

impl Eq for Window {}

impl Window {
    /// Consumed percentage, rounded and clamped for display.
    pub fn pct(&self) -> i32 {
        if !self.percent.is_finite() {
            return 0;
        }
        self.percent.round().clamp(0.0, 100.0) as i32
    }

    pub fn resets_at(&self) -> Option<DateTime<Utc>> {
        self.reset
    }

    /// The window as it stands at `now`: one whose reset has passed is empty.
    /// Cached figures are aged with this, so an idle night drains the bar
    /// without another request.
    pub fn aged(&self, now: DateTime<Utc>) -> Window {
        match self.reset {
            Some(reset) if reset <= now => Window {
                percent: 0.0,
                reset: None,
            },
            _ => self.clone(),
        }
    }
}

/// What the widget and TUI render for Cline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub plan: Option<String>,
    pub five_hour: Option<Window>,
    /// The weekly limit.
    pub seven_day: Option<Window>,
    /// The monthly limit.
    pub thirty_day: Option<Window>,
    pub period_end: Option<DateTime<Utc>>,
}

impl Snapshot {
    pub fn windows(&self) -> [(&'static str, Option<&Window>); 3] {
        [
            ("5h", self.five_hour.as_ref()),
            ("7d", self.seven_day.as_ref()),
            ("30d", self.thirty_day.as_ref()),
        ]
    }

    /// The most-exhausted window — the single number the bar leads with.
    pub fn worst_pct(&self) -> Option<i32> {
        self.windows()
            .into_iter()
            .filter_map(|(_, window)| window.map(Window::pct))
            .max()
    }
}

/// Parse `/api/v1/users/me/plan`.
pub fn parse_plan(value: &Value) -> Result<Plan, String> {
    let data = envelope(value, "plan")?;
    let plan = data
        .get("plan")
        .ok_or_else(|| "Cline plan response has no plan".to_string())?;

    Ok(Plan {
        name: plan
            .get("displayName")
            .or_else(|| plan.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        period_end: data
            .get("currentPeriodEnd")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
    })
}

/// Parse `/api/v1/users/me/plan/usage-limits` into the three windows.
///
/// A limit the server does not list stays `None` (a plan without ClinePass has
/// none). One that is listed must carry a numeric `percentUsed`, otherwise the
/// whole response is rejected: a bar that quietly shows 0 % is worse than one
/// that says it could not read the figure.
pub fn parse_limits(value: &Value) -> Result<[Option<Window>; 3], String> {
    let data = envelope(value, "usage limits")?;
    let limits = data
        .get("limits")
        .and_then(Value::as_array)
        .ok_or_else(|| "Cline usage-limits response has no limits".to_string())?;

    let mut windows: [Option<Window>; 3] = [None, None, None];
    for limit in limits {
        let Some(index) = limit
            .get("type")
            .and_then(Value::as_str)
            .and_then(|kind| LIMIT_TYPES.iter().position(|known| *known == kind))
        else {
            continue;
        };
        let percent = limit
            .get("percentUsed")
            .and_then(Value::as_f64)
            .filter(|percent| percent.is_finite())
            .ok_or_else(|| "Cline usage limit has no percentUsed".to_string())?;
        windows[index] = Some(Window {
            percent,
            reset: limit
                .get("resetsAt")
                .and_then(Value::as_str)
                .and_then(parse_timestamp),
        });
    }
    Ok(windows)
}

fn envelope<'a>(value: &'a Value, what: &str) -> Result<&'a Value, String> {
    if value.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(format!("Cline {what} response reports failure"));
    }
    value
        .get("data")
        .or(Some(value))
        .filter(|data| data.is_object())
        .ok_or_else(|| format!("Cline {what} response must be a JSON object"))
}

fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    text.parse::<DateTime<chrono::FixedOffset>>()
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().expect("RFC3339 timestamp")
    }

    #[test]
    fn parses_the_plan_fixture() {
        let value: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/cline/plan.json"))
                .expect("fixture JSON must be valid");

        let plan = parse_plan(&value).expect("plan must parse");
        assert_eq!(plan.name.as_deref(), Some("Cline Pass (Monthly)"));
        assert_eq!(plan.period_end, Some(at("2026-10-01T05:17:51Z")));
    }

    #[test]
    fn parses_the_real_usage_limits_response() {
        let value: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/cline/usage-limits.json"))
                .expect("fixture JSON must be valid");

        let [five, week, month] = parse_limits(&value).expect("limits must parse");
        let five = five.expect("5h");
        assert_eq!(five.pct(), 0);
        assert_eq!(five.reset, None);
        let week = week.expect("weekly");
        assert_eq!(week.pct(), 5);
        assert_eq!(week.reset, Some(at("2026-10-10T13:41:41.925010062Z")));
        let month = month.expect("monthly");
        assert_eq!(month.pct(), 2);
        assert_eq!(month.reset, Some(at("2026-11-02T13:41:41.927618642Z")));
    }

    #[test]
    fn a_plan_without_clinepass_has_no_limits() {
        let value: Value = serde_json::json!({"data": {"limits": []}, "success": true});
        assert_eq!(
            parse_limits(&value).expect("empty is valid"),
            [None, None, None]
        );
    }

    #[test]
    fn unknown_limit_types_are_ignored_but_a_listed_one_needs_a_percentage() {
        let ok = serde_json::json!({"data": {"limits": [
            {"type": "daily", "percentUsed": 90},
            {"type": "weekly", "percentUsed": 7}
        ]}});
        let [five, week, month] = parse_limits(&ok).expect("parses");
        assert!(five.is_none() && month.is_none());
        assert_eq!(week.expect("weekly").pct(), 7);

        let bad = serde_json::json!({"data": {"limits": [{"type": "weekly"}]}});
        assert!(parse_limits(&bad).is_err());
        let failed = serde_json::json!({"success": false});
        assert!(parse_limits(&failed).is_err());
    }

    #[test]
    fn a_window_past_its_reset_reads_as_empty() {
        let window = Window {
            percent: 40.0,
            reset: Some(at("2026-10-10T13:41:41Z")),
        };
        assert_eq!(window.aged(at("2026-10-10T13:41:40Z")).pct(), 40);
        let after = window.aged(at("2026-10-10T13:41:41Z"));
        assert_eq!(after.pct(), 0);
        assert_eq!(after.reset, None);
    }

    #[test]
    fn percentages_are_clamped_and_worst_pct_leads() {
        let snapshot = Snapshot {
            plan: None,
            five_hour: Some(Window {
                percent: 0.0,
                reset: None,
            }),
            seven_day: Some(Window {
                percent: 5.0,
                reset: None,
            }),
            thirty_day: Some(Window {
                percent: 130.0,
                reset: None,
            }),
            period_end: None,
        };
        assert_eq!(snapshot.worst_pct(), Some(100));
        assert_eq!(
            Window {
                percent: f64::NAN,
                reset: None
            }
            .pct(),
            0
        );
    }
}
