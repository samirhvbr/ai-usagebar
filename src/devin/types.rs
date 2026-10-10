//! Response projection for the Devin CLI's GetUserStatus quota endpoint.
//!
//! The API reports remaining percentages and Unix-second resets. ai-usagebar
//! stores consumed percentages in UsageWindow, so the conversion is explicit
//! and windows with no data at all remain absent.

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::usage::{DevinSnapshot, UsageWindow};

pub const DAILY_WINDOW: Duration = Duration::days(1);
pub const WEEKLY_WINDOW: Duration = Duration::days(7);

pub fn parse_response(bytes: &[u8]) -> Result<DevinSnapshot> {
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|_| AppError::Schema("Devin status response was not valid JSON".into()))?;
    let plan_status = root
        .get("userStatus")
        .and_then(Value::as_object)
        .and_then(|status| status.get("planStatus"))
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Schema("Devin status response omitted planStatus".into()))?;

    let recognized = [
        "dailyQuotaRemainingPercent",
        "dailyQuotaResetAtUnix",
        "weeklyQuotaRemainingPercent",
        "weeklyQuotaResetAtUnix",
        "overageBalanceMicros",
    ];
    if !recognized
        .iter()
        .any(|field| plan_status.contains_key(*field))
    {
        return Err(AppError::Schema(
            "Devin status response contained no recognized quota fields".into(),
        ));
    }

    let daily_remaining = optional_percent(plan_status, "dailyQuotaRemainingPercent")?;
    let daily_reset = optional_epoch_seconds(plan_status, "dailyQuotaResetAtUnix")?;
    let weekly_remaining = optional_percent(plan_status, "weeklyQuotaRemainingPercent")?;
    let weekly_reset = optional_epoch_seconds(plan_status, "weeklyQuotaResetAtUnix")?;
    let overage_balance_micros = optional_integer(plan_status, "overageBalanceMicros")?;

    Ok(DevinSnapshot {
        daily: quota_window(daily_remaining, daily_reset, DAILY_WINDOW),
        weekly: quota_window(weekly_remaining, weekly_reset, WEEKLY_WINDOW),
        overage_balance_micros,
    })
}

/// The response is protobuf JSON (Connect-RPC), and protojson omits scalar
/// fields that hold their zero value, so an exhausted window arrives as a
/// reset timestamp with no `...RemainingPercent` at all. A reset without a
/// remaining percentage is therefore read as 0% remaining (100% consumed)
/// rather than dropped, which would make an exhausted quota vanish. A live
/// response with the daily quota used up confirms it: `dailyQuotaResetAtUnix`
/// arrives and `dailyQuotaRemainingPercent` does not. With neither field the
/// window is genuinely absent.
fn quota_window(
    remaining_pct: Option<i32>,
    reset: Option<DateTime<Utc>>,
    duration: Duration,
) -> Option<UsageWindow> {
    let remaining_pct = match (remaining_pct, reset) {
        (Some(remaining), _) => remaining,
        (None, Some(_)) => 0,
        (None, None) => return None,
    };
    Some(UsageWindow {
        utilization_pct: 100 - remaining_pct,
        resets_at: reset,
        window_duration: duration,
    })
}

fn optional_percent(object: &serde_json::Map<String, Value>, field: &str) -> Result<Option<i32>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let raw = number.as_f64().ok_or_else(|| drift(field))?;
            percent_from_f64(raw, field).map(Some)
        }
        Some(Value::String(value)) => {
            let raw = value.parse::<f64>().map_err(|_| drift(field))?;
            percent_from_f64(raw, field).map(Some)
        }
        Some(_) => Err(drift(field)),
    }
}

fn percent_from_f64(raw: f64, field: &str) -> Result<i32> {
    if !raw.is_finite() || raw.fract() != 0.0 || !(0.0..=100.0).contains(&raw) {
        return Err(drift(field));
    }
    Ok(raw as i32)
}

fn optional_epoch_seconds(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<DateTime<Utc>>> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let seconds = integer_value(value).ok_or_else(|| drift(field))?;
    if seconds < 0 {
        return Err(drift(field));
    }
    DateTime::from_timestamp(seconds, 0)
        .map(Some)
        .ok_or_else(|| drift(field))
}

fn optional_integer(object: &serde_json::Map<String, Value>, field: &str) -> Result<Option<i64>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => integer_value(value).map(Some).ok_or_else(|| drift(field)),
    }
}

fn integer_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn drift(field: &str) -> AppError {
    AppError::Schema(format!(
        "Devin status field {field} had an unsupported value"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(plan_status: Value) -> Vec<u8> {
        serde_json::json!({"userStatus":{"planStatus":plan_status}})
            .to_string()
            .into_bytes()
    }

    #[test]
    fn numeric_and_string_remaining_values_become_consumed_windows_and_unix_resets() {
        let bytes = response(serde_json::json!({
            "dailyQuotaRemainingPercent": 100,
            "dailyQuotaResetAtUnix": "1791014400",
            "weeklyQuotaRemainingPercent": "69",
            "weeklyQuotaResetAtUnix": 1791100800,
            "overageBalanceMicros": "9168615"
        }));
        let snapshot = parse_response(&bytes).unwrap();
        let daily = snapshot.daily.unwrap();
        assert_eq!(daily.utilization_pct, 0);
        assert_eq!(daily.resets_at.unwrap().timestamp(), 1_791_014_400);
        assert_eq!(daily.window_duration, DAILY_WINDOW);
        let weekly = snapshot.weekly.unwrap();
        assert_eq!(weekly.utilization_pct, 31);
        assert_eq!(weekly.resets_at.unwrap().timestamp(), 1_791_100_800);
        assert_eq!(weekly.window_duration, WEEKLY_WINDOW);
        assert_eq!(snapshot.overage_balance_micros, Some(9_168_615));
    }

    #[test]
    fn windows_with_no_data_stay_absent_and_an_explicit_zero_is_exhausted() {
        let snapshot = parse_response(&response(serde_json::json!({
            "dailyQuotaRemainingPercent": null,
            "dailyQuotaResetAtUnix": null,
            "weeklyQuotaRemainingPercent": 0,
            "overageBalanceMicros": null
        })))
        .unwrap();
        assert!(snapshot.daily.is_none());
        let weekly = snapshot.weekly.unwrap();
        assert_eq!(weekly.utilization_pct, 100);
        assert_eq!(weekly.resets_at, None);
        assert_eq!(snapshot.overage_balance_micros, None);
    }

    /// The quota fields of a live response with the daily quota used up,
    /// account fields left out: the daily percentage is simply absent, the
    /// weekly one is a bare number and the int64 fields are strings.
    #[test]
    fn the_observed_exhausted_daily_response_reads_as_fully_used() {
        let snapshot = parse_response(&response(serde_json::json!({
            "planStart": "2026-10-03T13:51:30Z",
            "planEnd": "2026-11-03T13:51:39Z",
            "availablePromptCredits": -1,
            "weeklyQuotaRemainingPercent": 27,
            "overageBalanceMicros": "6165532",
            "dailyQuotaResetAtUnix": "1791360000",
            "weeklyQuotaResetAtUnix": "1791705600"
        })))
        .unwrap();
        let daily = snapshot.daily.unwrap();
        assert_eq!(daily.utilization_pct, 100);
        assert_eq!(daily.resets_at.unwrap().timestamp(), 1_791_360_000);
        let weekly = snapshot.weekly.unwrap();
        assert_eq!(weekly.utilization_pct, 73);
        assert_eq!(weekly.resets_at.unwrap().timestamp(), 1_791_705_600);
        assert_eq!(snapshot.overage_balance_micros, Some(6_165_532));
    }

    #[test]
    fn a_reset_without_a_remaining_percent_is_an_exhausted_window() {
        // protojson drops a zero-valued scalar, so 0% remaining arrives as a
        // reset timestamp alone. It must read as exhausted, not as absent.
        let snapshot = parse_response(&response(serde_json::json!({
            "dailyQuotaResetAtUnix": "1791014400",
            "weeklyQuotaRemainingPercent": "69",
            "weeklyQuotaResetAtUnix": "1791100800"
        })))
        .unwrap();
        let daily = snapshot.daily.unwrap();
        assert_eq!(daily.utilization_pct, 100);
        assert_eq!(daily.resets_at.unwrap().timestamp(), 1_791_014_400);
        assert_eq!(daily.window_duration, DAILY_WINDOW);
        assert_eq!(snapshot.weekly.unwrap().utilization_pct, 31);

        let weekly_only = parse_response(&response(serde_json::json!({
            "weeklyQuotaResetAtUnix": 1791100800
        })))
        .unwrap();
        assert!(weekly_only.daily.is_none());
        let weekly = weekly_only.weekly.unwrap();
        assert_eq!(weekly.utilization_pct, 100);
        assert_eq!(weekly.window_duration, WEEKLY_WINDOW);
    }

    #[test]
    fn a_remaining_percent_without_a_reset_keeps_its_value() {
        let snapshot = parse_response(&response(serde_json::json!({
            "dailyQuotaRemainingPercent": 40
        })))
        .unwrap();
        let daily = snapshot.daily.unwrap();
        assert_eq!(daily.utilization_pct, 60);
        assert_eq!(daily.resets_at, None);
    }

    #[test]
    fn invalid_percentages_resets_balances_and_envelopes_are_schema_errors() {
        for plan in [
            serde_json::json!({"dailyQuotaRemainingPercent": -1}),
            serde_json::json!({"dailyQuotaRemainingPercent": 101}),
            serde_json::json!({"dailyQuotaRemainingPercent": "1.5"}),
            serde_json::json!({"weeklyQuotaResetAtUnix": "soon"}),
            serde_json::json!({"dailyQuotaResetAtUnix": -1}),
            serde_json::json!({"overageBalanceMicros": 1.5}),
            serde_json::json!({"overageBalanceMicros": "unknown"}),
            serde_json::json!({"unrelated": 7}),
        ] {
            let error = parse_response(&response(plan)).unwrap_err();
            assert!(matches!(error, AppError::Schema(_)), "{error:?}");
        }
        assert!(parse_response(b"<html>secret</html>").is_err());
        assert!(parse_response(br#"{"userStatus":{}}"#).is_err());
    }
}
