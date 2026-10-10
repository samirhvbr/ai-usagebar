//! Devin renderer — daily/weekly consumed quota and optional overage balance.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::format::{placeholders, substitute, updated_at_hm, window_placeholders};
use crate::pacing::PaceSeverity;
use crate::pango::{color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, push_window, render_bordered};
use crate::usage::{DevinSnapshot, fmt_minor};
use crate::vendor::{RenderOpts, VendorId, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};

use super::fetch::FetchOutcome;

/// The balance arrives in microunits: six decimal places, rendered through
/// the shared minor-unit formatter. The USD reading is empirical, so the
/// wire value stays untouched in the snapshot and cache and the tooltip says
/// the currency contract is unverified.
pub const BALANCE_DECIMALS: u32 = 6;
pub const DEFAULT_FORMAT: &str = "D {devin_daily_pct}% · W {devin_weekly_pct}%";

pub fn build_placeholders(
    snapshot: &DevinSnapshot,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let daily = window_placeholders(snapshot.daily.as_ref(), opts, now);
    let weekly = window_placeholders(snapshot.weekly.as_ref(), opts, now);
    let balance = snapshot
        .overage_balance_micros
        .map(|micros| fmt_minor(micros, BALANCE_DECIMALS, None))
        .unwrap_or_default();
    placeholders(vec![
        ("icon", VendorId::Devin.bar_icon().to_string()),
        ("vendor_short", VendorId::Devin.short_name().to_string()),
        // There is no session window; preserve an empty value rather than
        // aliasing the daily quota to a misleading 5-hour session.
        ("session_pct", String::new()),
        ("session_reset", String::new()),
        ("session_elapsed", String::new()),
        ("weekly_pct", weekly.pct.clone()),
        ("weekly_reset", weekly.reset.clone()),
        ("weekly_elapsed", weekly.elapsed.clone()),
        ("plan", VendorId::Devin.display_name().to_string()),
        ("devin_daily_pct", daily.pct),
        ("devin_daily_reset", daily.reset),
        ("devin_daily_elapsed", daily.elapsed),
        ("devin_daily_pace", daily.ratio_pace),
        ("devin_daily_pace_indicator", daily.point_pace),
        ("devin_weekly_pct", weekly.pct),
        ("devin_weekly_reset", weekly.reset),
        ("devin_weekly_elapsed", weekly.elapsed),
        ("devin_weekly_pace", weekly.ratio_pace),
        ("devin_weekly_pace_indicator", weekly.point_pace),
        ("devin_overage_balance", balance),
    ])
}

pub fn severity(snapshot: &DevinSnapshot) -> PaceSeverity {
    snapshot
        .daily
        .iter()
        .chain(snapshot.weekly.iter())
        .map(|window| window.utilization_pct)
        .max()
        .map(severity_for)
        .unwrap_or(PaceSeverity::Low)
}

pub fn render(
    outcome: &VendorOutcome,
    snapshot: &DevinSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let severity = severity(snapshot);
    let class = Class::from(severity);
    let format = opts
        .format
        .clone()
        .unwrap_or_else(|| default_format(snapshot));
    let values = build_placeholders(snapshot, opts, now);
    let mut text = substitute(&format, &values);
    if outcome.stale {
        text.push_str(" ⏸");
    }
    let icon = opts
        .icon
        .as_deref()
        .filter(|icon| !icon.is_empty())
        .map(|icon| format!("{icon} "))
        .unwrap_or_default();
    let bar = color_span(severity_color(severity, theme), &format!("{icon}{text}"));
    let tooltip = opts
        .tooltip_format
        .as_deref()
        .map(|format| substitute(format, &values))
        .unwrap_or_else(|| render_tooltip(outcome, snapshot, theme, now));
    WaybarOutput {
        text: bar,
        tooltip,
        class,
    }
}

fn default_format(snapshot: &DevinSnapshot) -> String {
    match (snapshot.daily.as_ref(), snapshot.weekly.as_ref()) {
        (Some(_), Some(_)) => DEFAULT_FORMAT.to_string(),
        (Some(_), None) => "D {devin_daily_pct}% · {devin_daily_reset}".into(),
        (None, Some(_)) => "W {devin_weekly_pct}% · {devin_weekly_reset}".into(),
        (None, None) => snapshot
            .overage_balance_micros
            .map(|micros| fmt_minor(micros, BALANCE_DECIMALS, None))
            .unwrap_or_else(|| VendorId::Devin.display_name().to_string()),
    }
}

fn render_tooltip(
    outcome: &VendorOutcome,
    snapshot: &DevinSnapshot,
    theme: &Theme,
    now: DateTime<Utc>,
) -> String {
    let blue = &theme.blue;
    let dim = &theme.dim;
    let mut lines = vec![
        TooltipLine::Center(format!(
            "<span font_weight='bold' foreground='{blue}'>{}</span>",
            escape(VendorId::Devin.display_name())
        )),
        TooltipLine::Sep,
        TooltipLine::Body(String::new()),
    ];
    if let Some(daily) = snapshot.daily.as_ref() {
        push_window(&mut lines, "  Daily quota", daily, theme, now, None);
    }
    if let Some(weekly) = snapshot.weekly.as_ref() {
        if snapshot.daily.is_some() {
            lines.push(TooltipLine::Body(String::new()));
        }
        push_window(&mut lines, "  Weekly quota", weekly, theme, now, None);
    }
    if snapshot.daily.is_none()
        && snapshot.weekly.is_none()
        && snapshot.overage_balance_micros.is_none()
    {
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>  no quota fields reported</span>"
        )));
    }
    if let Some(balance) = snapshot.overage_balance_micros {
        lines.push(TooltipLine::Body(String::new()));
        lines.push(TooltipLine::Body(format!(
            "  Overage balance: {}",
            escape(&fmt_minor(balance, BALANCE_DECIMALS, None))
        )));
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>  Microunit-to-USD display matches the tested account; currency contract is unverified.</span>"
        )));
    }
    if let Some((code, message)) = outcome.last_error.as_ref() {
        if *code != 0 {
            lines.push(TooltipLine::Body(String::new()));
            lines.push(TooltipLine::Sep);
            lines.push(TooltipLine::Body(format!(
                " <span foreground='{}'>  HTTP {code}</span>",
                theme.orange
            )));
        }
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>  {}</span>",
            escape(message)
        )));
    }
    let updated = updated_at_hm(now, outcome.cache_age);
    lines.push(TooltipLine::Body(String::new()));
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{dim}'>  Updated {updated}</span>"
    )));
    render_bordered(&lines, theme)
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(outcome: FetchOutcome) -> Self {
        outcome.map(crate::usage::VendorSnapshot::Devin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::UsageWindow;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_791_014_400, 0).unwrap()
    }

    fn window(pct: i32) -> UsageWindow {
        UsageWindow {
            utilization_pct: pct,
            resets_at: Some(now() + chrono::Duration::hours(2)),
            window_duration: chrono::Duration::days(1),
        }
    }

    fn snapshot() -> DevinSnapshot {
        DevinSnapshot {
            daily: Some(window(0)),
            weekly: Some(UsageWindow {
                utilization_pct: 31,
                resets_at: Some(now() + chrono::Duration::days(2)),
                window_duration: chrono::Duration::days(7),
            }),
            overage_balance_micros: Some(9_168_615),
        }
    }

    fn opts() -> RenderOpts {
        RenderOpts {
            format: None,
            tooltip_format: None,
            icon: None,
            pace_tolerance: 5,
            format_pace_color: false,
            tooltip_pace_pts: false,
        }
    }

    #[test]
    fn balance_keeps_six_decimal_microunits_with_the_sign_outside_the_symbol() {
        // Pinned to the output of the removed Devin-only formatter, which the
        // shared `fmt_minor` reproduces byte for byte.
        for (micros, expected) in [
            (9_168_615, "$9.168615"),
            (0, "$0.000000"),
            (500, "$0.000500"),
            (1_000_000, "$1.000000"),
            (-1, "-$0.000001"),
            (-9_168_615, "-$9.168615"),
            (i64::MAX, "$9223372036854.775807"),
            (i64::MIN, "-$9223372036854.775808"),
        ] {
            assert_eq!(fmt_minor(micros, BALANCE_DECIMALS, None), expected);
        }
    }

    #[test]
    fn the_plan_label_is_the_shared_display_name() {
        let values = build_placeholders(&snapshot(), &opts(), now());
        assert_eq!(values.get("plan").map(String::as_str), Some("Devin"));
        let balance_only = DevinSnapshot {
            daily: None,
            weekly: None,
            overage_balance_micros: None,
        };
        assert_eq!(default_format(&balance_only), "Devin");
    }

    #[test]
    fn renderer_uses_daily_and_weekly_placeholders_without_faking_a_session() {
        let snapshot = snapshot();
        let values = build_placeholders(&snapshot, &opts(), now());
        assert_eq!(values.get("devin_daily_pct").map(String::as_str), Some("0"));
        assert_eq!(
            values.get("devin_weekly_pct").map(String::as_str),
            Some("31")
        );
        assert_eq!(values.get("session_pct").map(String::as_str), Some(""));
        assert_eq!(
            values.get("devin_overage_balance").map(String::as_str),
            Some("$9.168615")
        );
    }

    #[test]
    fn render_shows_the_currency_caveat_and_exact_balance() {
        let snapshot = snapshot();
        let outcome = VendorOutcome {
            snapshot: crate::usage::VendorSnapshot::Devin(snapshot.clone()),
            stale: false,
            last_error: None,
            cache_age: Some(std::time::Duration::ZERO),
        };
        let rendered = render(&outcome, &snapshot, &Theme::default(), &opts(), now());
        assert!(rendered.text.contains("D 0%") && rendered.text.contains("W 31%"));
        assert!(rendered.tooltip.contains("$9.168615"));
        assert!(rendered.tooltip.contains("currency contract is unverified"));
    }
}
