//! Wire types for the Anthropic Admin API cost report
//! (`GET /v1/organizations/cost_report`).
//!
//! Confirmed against the official docs
//! (<https://platform.claude.com/docs/en/api/admin-api/usage-cost/get-cost-report>):
//! `amount` is a decimal STRING in the currency's LOWEST unit (cents) —
//! `"123.45"` USD represents `$1.23` — so divide by 100 for dollars. There is
//! no API for the remaining prepaid credit balance (Console dashboard only), so
//! this vendor reports **month-to-date spend** instead.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CostReport {
    pub data: Vec<Bucket>,
    pub has_more: bool,
    pub next_page: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Bucket {
    pub results: Vec<CostResult>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CostResult {
    /// Cost in the currency's lowest unit (cents), as a decimal string.
    pub amount: String,
}

/// Sum every result's cents on one page and return dollars.
pub fn page_dollars(report: &CostReport) -> f64 {
    let cents: f64 = report
        .data
        .iter()
        .flat_map(|b| b.results.iter())
        .map(|r| r.amount.trim().parse::<f64>().unwrap_or(0.0))
        .sum();
    cents / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verbatim shape from the docs (amount in cents).
    const BODY: &str = r#"{
      "data": [
        { "starting_at": "2026-07-01T00:00:00Z", "ending_at": "2026-07-02T00:00:00Z",
          "results": [
            { "amount": "100.0", "currency": "USD", "cost_type": "tokens" },
            { "amount": "34.5", "currency": "USD", "cost_type": "web_search" }
          ] }
      ],
      "has_more": false,
      "next_page": null
    }"#;

    #[test]
    fn sums_cents_and_converts_to_dollars() {
        let report: CostReport = serde_json::from_str(BODY).unwrap();
        // 100.0 + 34.5 = 134.5 cents = $1.345
        assert!((page_dollars(&report) - 1.345).abs() < 1e-9);
        assert!(!report.has_more);
    }

    #[test]
    fn empty_report_is_zero() {
        let report: CostReport = serde_json::from_str("{}").unwrap();
        assert_eq!(page_dollars(&report), 0.0);
    }
}
