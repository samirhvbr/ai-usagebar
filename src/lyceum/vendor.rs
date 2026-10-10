//! Lyceum widget: USD balance, without an invented quota meter.
use crate::format::{placeholders, substitute, updated_at_hm, usd};
use crate::pango::{color_span, escape};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, render_bordered};
use crate::usage::LyceumSnapshot;
use crate::vendor::{RenderOpts, VendorId, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

impl From<super::fetch::FetchOutcome> for VendorOutcome {
    fn from(outcome: super::fetch::FetchOutcome) -> Self {
        outcome.map(crate::usage::VendorSnapshot::Lyceum)
    }
}

pub const DEFAULT_FORMAT: &str = "{lyceum_balance}";
pub fn build_placeholders(s: &LyceumSnapshot) -> HashMap<&'static str, String> {
    placeholders(vec![
        ("icon", VendorId::Lyceum.short_name().into()),
        ("vendor_short", VendorId::Lyceum.short_name().into()),
        ("session_pct", "".into()),
        ("session_reset", "—".into()),
        ("weekly_pct", "".into()),
        ("weekly_reset", "—".into()),
        ("plan", VendorId::Lyceum.display_name().into()),
        ("lyceum_balance", usd(s.available_credits)),
        ("lyceum_used", usd(s.used_credits)),
        ("lyceum_remaining", usd(s.remaining_credits)),
    ])
}
pub fn render(
    outcome: &VendorOutcome,
    snap: &LyceumSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let values = build_placeholders(snap);
    let format = opts.format.as_deref().unwrap_or(DEFAULT_FORMAT);
    let text = substitute(format, &values);
    let icon = opts
        .icon
        .as_deref()
        .filter(|i| !i.is_empty())
        .map(|i| format!("{i} "))
        .unwrap_or_default();
    let tooltip = if let Some(custom) = opts.tooltip_format.as_deref() {
        substitute(custom, &values)
    } else {
        let lines = vec![
            TooltipLine::Center(format!(
                "<span font_weight='bold' foreground='{}'>Lyceum</span>",
                theme.blue
            )),
            TooltipLine::Sep,
            TooltipLine::Body(format!(
                "Available balance: {}",
                escape(&usd(snap.available_credits))
            )),
            TooltipLine::Body(format!("Amount used: {}", escape(&usd(snap.used_credits)))),
            TooltipLine::Body(format!("Updated {}", updated_at_hm(now, outcome.cache_age))),
        ];
        render_bordered(&lines, theme)
    };
    WaybarOutput {
        text: color_span(&theme.fg, &format!("{icon}{text}")),
        tooltip,
        class: Class::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_placeholders_render_us_dollars() {
        let snapshot = LyceumSnapshot {
            available_credits: 42.5,
            used_credits: 7.5,
            total_credits_used: 7.5,
            remaining_credits: 42.5,
            monthly_free_credits: 0.0,
            purchased_credits: 50.0,
        };
        let values = build_placeholders(&snapshot);
        assert_eq!(values["lyceum_balance"], "$42.50");
        assert_eq!(values["lyceum_used"], "$7.50");
        assert_eq!(values["lyceum_remaining"], "$42.50");
    }
}
