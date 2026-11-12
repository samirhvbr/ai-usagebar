use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::usage::{SyntheticSnapshot, UsageWindow, WeeklyCredits};

/// Synthetic `/v2/quotas` payload. The weekly window is the sole metric:
/// `planWeeklyLimit` carries it as a percentage (subscription plans) and
/// `weeklyTokenLimit` as a credit budget (usage-based plans). The legacy
/// five-hour `subscription` counter is still echoed by the API but is no
/// longer tracked, so it is deliberately ignored.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    /// Preferred by Synthetic's own tooling when both objects appear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_weekly_limit: Option<PlanWeeklyLimit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weekly_token_limit: Option<WeeklyTokenLimit>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanWeeklyLimit {
    pub next_regen_at: String,
    pub percent_remaining: f64,
    pub next_regen_percent: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyTokenLimit {
    pub next_regen_at: String,
    pub percent_remaining: f64,
    /// The API reports credit amounts as numeric strings ("24.72").
    #[serde(deserialize_with = "de_credits")]
    pub max_credits: f64,
    #[serde(deserialize_with = "de_credits")]
    pub remaining_credits: f64,
    #[serde(deserialize_with = "de_credits")]
    pub next_regen_credits: f64,
}

struct WeeklyFigures {
    percent_remaining: f64,
    renews_at: DateTime<Utc>,
    next_regen_percent: Option<f64>,
    credits: Option<WeeklyCredits>,
}

impl WeeklyFigures {
    fn snapshot(self) -> SyntheticSnapshot {
        let utilization_pct = (100.0 - self.percent_remaining).round() as i32;
        let utilization_precise = ((100.0 - self.percent_remaining) * 100.0).round() / 100.0;
        SyntheticSnapshot {
            plan: None,
            weekly: UsageWindow {
                utilization_pct,
                resets_at: Some(self.renews_at),
                // The API documents the next regeneration tick, not a fixed
                // cycle length: the allowance refills ~2% every 202 minutes
                // instead of resetting wholesale. Zero deliberately disables
                // guessed pacing.
                window_duration: chrono::Duration::zero(),
            },
            credits: self.credits,
            next_regen_percent: self.next_regen_percent,
            utilization_precise,
        }
    }
}

impl PlanWeeklyLimit {
    fn figures(&self) -> Result<WeeklyFigures> {
        Ok(WeeklyFigures {
            percent_remaining: parse_percent(self.percent_remaining)?,
            renews_at: parse_renews_at(&self.next_regen_at)?,
            next_regen_percent: Some(parse_regens(self.next_regen_percent)?),
            credits: None,
        })
    }
}

impl WeeklyTokenLimit {
    fn figures(&self) -> Result<WeeklyFigures> {
        let (max, remaining, next_regen) = (
            self.max_credits,
            self.remaining_credits,
            self.next_regen_credits,
        );
        if !max.is_finite()
            || max <= 0.0
            || !remaining.is_finite()
            || remaining < 0.0
            || !next_regen.is_finite()
            || next_regen < 0.0
        {
            return Err(AppError::Schema(
                "Synthetic weekly credit budget contains an invalid amount".into(),
            ));
        }
        Ok(WeeklyFigures {
            percent_remaining: parse_percent(self.percent_remaining)?,
            renews_at: parse_renews_at(&self.next_regen_at)?,
            next_regen_percent: None,
            credits: Some(WeeklyCredits {
                used: (max - remaining).max(0.0),
                max,
                next_regen,
            }),
        })
    }
}

impl Response {
    pub fn snapshot(&self) -> Result<SyntheticSnapshot> {
        // Synthetic's own tooling prefers the percentage-only plan object
        // when both ride along; keep that precedence and attach the credit
        // figures from the token variant when they are usable.
        let mut figures = match (&self.plan_weekly_limit, &self.weekly_token_limit) {
            (Some(plan), _) => plan.figures()?,
            (None, Some(tokens)) => tokens.figures()?,
            (None, None) => {
                return Err(AppError::Schema(
                    "Synthetic quota response carries no weekly limit".into(),
                ));
            }
        };
        if figures.credits.is_none()
            && let Some(tokens) = &self.weekly_token_limit
            && let Ok(extra) = tokens.figures()
        {
            figures.credits = extra.credits;
        }
        Ok(figures.snapshot())
    }
}

fn parse_percent(value: f64) -> Result<f64> {
    if !value.is_finite() || value < 0.0 {
        return Err(AppError::Schema(
            "Synthetic weekly quota contains an invalid remaining percentage".into(),
        ));
    }
    Ok(value.clamp(0.0, 100.0))
}

fn parse_regens(value: f64) -> Result<f64> {
    if !value.is_finite() || value < 0.0 {
        return Err(AppError::Schema(
            "Synthetic weekly quota contains an invalid regeneration amount".into(),
        ));
    }
    Ok(value)
}

fn parse_renews_at(raw: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| AppError::Schema("Synthetic weekly quota renewal is invalid".into()))
}

/// Credit amounts arrive as numeric strings and may carry currency
/// decoration; accept a bare JSON number as well.
fn de_credits<'de, D>(deserializer: D) -> std::result::Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error as _;
    match Value::deserialize(deserializer)? {
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| D::Error::custom("credit amount is not a finite number")),
        Value::String(raw) => {
            let cleaned = raw.trim().trim_start_matches('$').replace(',', "");
            cleaned
                .parse::<f64>()
                .map_err(|_| D::Error::custom("credit amount is not a number"))
        }
        _ => Err(D::Error::custom(
            "credit amount must be a number or numeric string",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = r#"{"planWeeklyLimit":{"nextRegenAt":"2026-10-27T01:09:02.000Z","percentRemaining":68.97415291835814,"nextRegenPercent":2}}"#;
    const TOKENS: &str = r#"{"weeklyTokenLimit":{"nextRegenAt":"2026-10-27T01:09:02.000Z","percentRemaining":82.4,"maxCredits":"30.00","remainingCredits":"24.72","nextRegenCredits":"0.60"}}"#;

    #[test]
    fn plan_shape_projects_the_documented_weekly_example() {
        // Captured verbatim from the live /v2/quotas response.
        let response: Response = serde_json::from_str(PLAN).unwrap();
        let snapshot = response.snapshot().unwrap();
        assert_eq!(snapshot.weekly.utilization_pct, 31);
        assert_eq!(snapshot.utilization_precise, 31.03);
        assert_eq!(
            snapshot.weekly.resets_at,
            Some("2026-10-27T01:09:02.000Z".parse().unwrap())
        );
        assert_eq!(snapshot.weekly.window_duration, chrono::Duration::zero());
        assert_eq!(snapshot.credits, None);
        assert_eq!(snapshot.next_regen_percent, Some(2.0));
    }

    #[test]
    fn token_shape_reports_the_credit_budget() {
        let response: Response = serde_json::from_str(TOKENS).unwrap();
        let snapshot = response.snapshot().unwrap();
        assert_eq!(snapshot.weekly.utilization_pct, 18);
        let credits = snapshot.credits.expect("credit figures");
        assert!((credits.used - 5.28).abs() < 1e-9);
        assert_eq!(credits.max, 30.0);
        assert_eq!(credits.next_regen, 0.6);
    }

    #[test]
    fn plan_shape_wins_but_adopts_token_credits() {
        let response: Response = serde_json::from_str(
            r#"{"planWeeklyLimit":{"nextRegenAt":"2026-10-27T01:09:02.000Z","percentRemaining":68.97415291835814,"nextRegenPercent":2},
                "weeklyTokenLimit":{"nextRegenAt":"2026-10-27T05:00:00.000Z","percentRemaining":82.4,"maxCredits":"30.00","remainingCredits":"24.72","nextRegenCredits":"0.60"}}"#,
        )
        .unwrap();
        let snapshot = response.snapshot().unwrap();
        // The plan object's percentage and regeneration clock win...
        assert_eq!(snapshot.weekly.utilization_pct, 31);
        assert_eq!(
            snapshot.weekly.resets_at,
            Some("2026-10-27T01:09:02.000Z".parse().unwrap())
        );
        assert_eq!(snapshot.next_regen_percent, Some(2.0));
        // ...while the token variant still supplies the credit figures.
        let credits = snapshot.credits.expect("credit figures");
        assert!((credits.used - 5.28).abs() < 1e-9);
        assert_eq!(credits.max, 30.0);
    }

    #[test]
    fn invalid_or_ambiguous_numbers_are_rejected() {
        for raw in [
            r#"{"planWeeklyLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":-1,"nextRegenPercent":2}}"#,
            r#"{"planWeeklyLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":50,"nextRegenPercent":-2}}"#,
            r#"{"planWeeklyLimit":{"nextRegenAt":"not-a-time","percentRemaining":50,"nextRegenPercent":2}}"#,
            r#"{"weeklyTokenLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":50,"maxCredits":"0","remainingCredits":"1","nextRegenCredits":"0.60"}}"#,
            r#"{"weeklyTokenLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":50,"maxCredits":"30.00","remainingCredits":"-1","nextRegenCredits":"0.60"}}"#,
            r#"{"subscription":{"limit":135,"requests":27,"renewsAt":"2026-11-01T00:00:00Z"}}"#,
            r#"{"error":"unauthorized"}"#,
        ] {
            let result = serde_json::from_str::<Response>(raw)
                .map_err(AppError::from)
                .and_then(|response| response.snapshot());
            assert!(result.is_err(), "accepted {raw}");
        }
    }

    #[test]
    fn over_quota_remaining_is_bounded_for_frontends() {
        let response: Response = serde_json::from_str(
            r#"{"planWeeklyLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":120,"nextRegenPercent":2}}"#,
        )
        .unwrap();
        assert_eq!(response.snapshot().unwrap().weekly.utilization_pct, 0);

        let response: Response = serde_json::from_str(
            r#"{"planWeeklyLimit":{"nextRegenAt":"2026-11-01T00:00:00Z","percentRemaining":-0.4,"nextRegenPercent":2}}"#,
        )
        .unwrap();
        // Negative remaining is invalid wire data, not just over quota.
        assert!(response.snapshot().is_err());
    }
}
