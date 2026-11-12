use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::usage::{MuseSnapshot, UsageWindow};

pub const UNAVAILABLE: &str = "Muse Code did not report subscription quota (it may omit both windows while the session is idle); remaining allowance is unknown";

/// Only quota-bearing fields: serde skips api_key, user_email, and every
/// other account field without retaining them in the parsed record.
#[derive(Clone, Deserialize, Serialize)]
pub struct Response {
    pub is_subs_active: bool,
    pub subs_tier_name: Option<String>,
    pub subs_usage: Option<Quota>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Quota {
    pub window: Option<Window>,
    pub weekly: Option<Window>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Window {
    pub used_percent: f64,
    pub resets_at: i64,
    pub window_duration_mins: Option<i64>,
}

impl Response {
    pub fn snapshot(&self, now: DateTime<Utc>) -> Result<MuseSnapshot> {
        if !self.is_subs_active {
            return Err(AppError::Credentials(
                "No active Muse Code subscription on this login".into(),
            ));
        }
        let quota = self
            .subs_usage
            .as_ref()
            .ok_or_else(|| AppError::Other(UNAVAILABLE.into()))?;
        let window = |raw: &Window, minutes: i64| -> Result<Option<UsageWindow>> {
            if !raw.used_percent.is_finite()
                || raw.used_percent < 0.0
                || !(1..=525600).contains(&minutes)
            {
                return Err(AppError::Schema(
                    "Muse Code quota contains an invalid percentage or duration".into(),
                ));
            }
            let reset = DateTime::from_timestamp(raw.resets_at, 0)
                .filter(|_| raw.resets_at > 0)
                .ok_or_else(|| {
                    AppError::Schema("Muse Code quota contains an invalid reset".into())
                })?;
            // An expired observation cannot describe the next window.
            Ok((reset > now).then_some(UsageWindow {
                utilization_pct: raw.used_percent.round().min(i32::MAX as f64) as i32,
                resets_at: Some(reset),
                window_duration: chrono::Duration::minutes(minutes),
            }))
        };
        let session = quota
            .window
            .as_ref()
            .map(|raw| {
                let minutes = raw
                    .window_duration_mins
                    .ok_or_else(|| AppError::Schema("Muse Code session duration missing".into()))?;
                window(raw, minutes)
            })
            .transpose()?
            .flatten();
        let weekly = quota
            .weekly
            .as_ref()
            .map(|raw| window(raw, 7 * 24 * 60))
            .transpose()?
            .flatten();
        if session.is_none() && weekly.is_none() {
            return Err(AppError::Other(UNAVAILABLE.into()));
        }
        Ok(MuseSnapshot {
            plan: crate::display::sanitize_untrusted_field(
                self.subs_tier_name.as_deref().unwrap_or("Muse Code"),
            ),
            session,
            weekly,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1791936000, 0).unwrap()
    }
    #[test]
    fn wire_seconds_and_dynamic_duration_project_to_report_windows() {
        let response: Response = serde_json::from_value(serde_json::json!({
            "is_subs_active":true,"subs_tier_name":"Muse Code Power Usage",
            "api_key":"SecretCanary-9348","user_email":"secret@example.test",
            "subs_usage":{"window":{"used_percent":38.5,"resets_at":1791942677,"window_duration_mins":300},
                          "weekly":{"used_percent":34,"resets_at":1792368000}}
        })).unwrap();
        let snap = response.snapshot(now()).unwrap();
        assert_eq!(snap.session.as_ref().unwrap().utilization_pct, 39);
        assert_eq!(
            snap.weekly.as_ref().unwrap().window_duration,
            chrono::Duration::days(7)
        );
        assert_eq!(
            snap.session.unwrap().window_duration,
            chrono::Duration::hours(5)
        );
        let cached = serde_json::to_string(&response).unwrap();
        assert!(!cached.contains("SecretCanary") && !cached.contains("secret@example"));
    }
    #[test]
    fn absent_quota_and_expired_observations_are_unknown_not_zero() {
        for usage in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!({
            "weekly":{"used_percent":34,"resets_at":1791935000}}),
        ] {
            let response: Response = serde_json::from_value(
                serde_json::json!({"is_subs_active":true,"subs_usage":usage}),
            )
            .unwrap();
            assert!(response.snapshot(now()).is_err());
        }
    }
}
