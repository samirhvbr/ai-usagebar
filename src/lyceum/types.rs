//! Typed response from Lyceum's customer-facing billing credits endpoint.
use crate::error::Result;
use crate::usage::{LyceumSnapshot, finite_amount};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Credits {
    pub available_credits: f64,
    pub used_credits: f64,
    pub total_credits_used: f64,
    pub remaining_credits: f64,
    #[serde(default)]
    pub monthly_free_credits: f64,
    #[serde(default)]
    pub purchased_credits: f64,
    #[serde(default)]
    pub signup_grant_claimed_at: Option<String>,
}
impl Credits {
    pub fn into_snapshot(self) -> Result<LyceumSnapshot> {
        for (field, amount) in [
            ("available_credits", self.available_credits),
            ("used_credits", self.used_credits),
            ("total_credits_used", self.total_credits_used),
            ("remaining_credits", self.remaining_credits),
            ("monthly_free_credits", self.monthly_free_credits),
            ("purchased_credits", self.purchased_credits),
        ] {
            finite_amount("lyceum", field, amount)?;
        }
        Ok(LyceumSnapshot {
            available_credits: self.available_credits,
            used_credits: self.used_credits,
            total_credits_used: self.total_credits_used,
            remaining_credits: self.remaining_credits,
            monthly_free_credits: self.monthly_free_credits,
            purchased_credits: self.purchased_credits,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn documented_fields_parse_and_extra_fields_are_ignored() {
        let body = r#"{"available_credits":123,"used_credits":12,"total_credits_used":12,"remaining_credits":123,"monthly_free_credits":0,"purchased_credits":100,"signup_grant_claimed_at":null,"future_field":true}"#;
        let snap = serde_json::from_str::<Credits>(body)
            .unwrap()
            .into_snapshot()
            .unwrap();
        assert_eq!(snap.available_credits, 123.0);
        assert_eq!(snap.purchased_credits, 100.0);
    }
    #[test]
    fn negative_credit_balances_are_preserved() {
        let credits = Credits {
            available_credits: -1.25,
            used_credits: 2.0,
            total_credits_used: 2.0,
            remaining_credits: -1.25,
            monthly_free_credits: 0.0,
            purchased_credits: 1.0,
            signup_grant_claimed_at: None,
        };
        let snapshot = credits
            .into_snapshot()
            .expect("negative balance is valid data");
        assert_eq!(snapshot.available_credits, -1.25);
        assert_eq!(snapshot.remaining_credits, -1.25);
    }
    #[test]
    fn missing_or_non_finite_fields_are_rejected() {
        assert!(serde_json::from_str::<Credits>(r#"{"available_credits":1}"#).is_err());
        let mut c=serde_json::from_str::<Credits>(r#"{"available_credits":1,"used_credits":0,"total_credits_used":0,"remaining_credits":1}"#).unwrap();
        c.available_credits = f64::NAN;
        assert!(c.into_snapshot().is_err());
    }
}
