use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::format::{placeholders, substitute, updated_at_hm, window_placeholders};
use crate::pacing::PaceSeverity;
use crate::pango::{color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line, push_window, render_bordered};
use crate::usage::SyntheticSnapshot;
use crate::vendor::{RenderOpts, VendorId, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};

impl From<super::fetch::FetchOutcome> for VendorOutcome {
    fn from(outcome: super::fetch::FetchOutcome) -> Self {
        outcome.map(crate::usage::VendorSnapshot::Synthetic)
    }
}

pub fn build_placeholders(
    snapshot: &SyntheticSnapshot,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let window = window_placeholders(Some(&snapshot.weekly), opts, now);
    placeholders([
        ("icon", VendorId::Synthetic.bar_icon().into()),
        ("vendor_short", VendorId::Synthetic.short_name().into()),
        (
            "plan",
            snapshot
                .plan
                .clone()
                .unwrap_or_else(|| "Synthetic".to_string()),
        ),
        ("session_pct", window.pct.clone()),
        ("session_reset", window.reset.clone()),
        ("weekly_pct", window.pct.clone()),
        ("weekly_reset", window.reset.clone()),
        ("synthetic_pct", window.pct),
        ("synthetic_reset", window.reset),
        (
            "synthetic_requests",
            snapshot
                .credits
                .as_ref()
                .map(|c| format_number(c.used))
                .unwrap_or_default(),
        ),
        (
            "synthetic_limit",
            snapshot
                .credits
                .as_ref()
                .map(|c| format_number(c.max))
                .unwrap_or_default(),
        ),
    ])
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

pub fn severity(snapshot: &SyntheticSnapshot) -> PaceSeverity {
    severity_for(snapshot.weekly.utilization_pct)
}

pub fn render(
    outcome: &VendorOutcome,
    snapshot: &SyntheticSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let mut values = build_placeholders(snapshot, opts, now);
    for value in values.values_mut() {
        *value = escape(value);
    }
    let mut text = substitute(
        opts.format.as_deref().unwrap_or("{synthetic_pct}%"),
        &values,
    );
    if let Some(icon) = &opts.icon {
        text = format!("{} {text}", escape(icon));
    }
    if outcome.stale {
        text.push_str(" ⏸");
    }
    let tooltip = opts
        .tooltip_format
        .as_deref()
        .map(|format| substitute(format, &values))
        .unwrap_or_else(|| {
            let mut lines = vec![Line::Center("Synthetic".into()), Line::Sep];
            push_window(
                &mut lines,
                "Weekly credits",
                &snapshot.weekly,
                theme,
                now,
                None,
            );
            if let Some(credits) = &snapshot.credits {
                lines.push(Line::Body(format!(
                    "Credits {} / {}",
                    escape(&format_number(credits.used)),
                    escape(&format_number(credits.max))
                )));
            }
            let regen = match (&snapshot.credits, snapshot.next_regen_percent) {
                (Some(credits), _) if credits.next_regen > 0.0 => Some(format!(
                    "Next regen +{} credits",
                    escape(&format_number(credits.next_regen))
                )),
                (_, Some(percent)) if percent > 0.0 => {
                    Some(format!("Next regen +{}%", escape(&format_number(percent))))
                }
                _ => None,
            };
            if let Some(regen) = regen {
                lines.push(Line::Body(regen));
            }
            if snapshot.credits.is_none() {
                lines.push(Line::Body(
                    "Plan reports the weekly budget as a percentage only".into(),
                ));
            }
            if outcome.stale {
                lines.push(Line::Body(
                    "⏸ Previous quota observation; current allowance is unverified".into(),
                ));
            }
            if let Some((_, error)) = &outcome.last_error {
                lines.push(Line::Body(escape(error)));
            }
            lines.push(Line::Body(format!(
                "Updated {}",
                updated_at_hm(now, outcome.cache_age)
            )));
            render_bordered(&lines, theme)
        });
    WaybarOutput {
        text: color_span(severity_color(severity(snapshot), theme), &text),
        tooltip,
        class: Class::from(severity(snapshot)),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::usage::UsageWindow;

    #[test]
    fn renderer_exports_documented_count_and_percent() {
        let now = DateTime::from_timestamp(1_791_014_400, 0).unwrap();
        let snapshot = SyntheticSnapshot {
            plan: Some("Standard".to_string()),
            weekly: UsageWindow {
                utilization_pct: 31,
                resets_at: Some(now + chrono::Duration::days(2)),
                window_duration: chrono::Duration::zero(),
            },
            credits: Some(crate::usage::WeeklyCredits {
                used: 5.28,
                max: 30.0,
                next_regen: 0.6,
            }),
            next_regen_percent: Some(2.0),
            utilization_precise: 30.6,
        };
        let opts = RenderOpts {
            format: None,
            tooltip_format: None,
            icon: None,
            pace_tolerance: 5,
            format_pace_color: false,
            tooltip_pace_pts: false,
        };
        let outcome = VendorOutcome {
            snapshot: crate::usage::VendorSnapshot::Synthetic(snapshot.clone()),
            stale: false,
            last_error: None,
            cache_age: Some(Duration::from_secs(1)),
        };
        let rendered = render(&outcome, &snapshot, &Theme::default(), &opts, now);
        assert!(rendered.text.contains("31%"));
        assert!(rendered.tooltip.contains("Credits 5.28 / 30"));
        assert!(rendered.tooltip.contains("Next regen +0.60 credits"));
        // The configured plan tier names the entry.
        let placeholders = build_placeholders(&snapshot, &opts, now);
        assert_eq!(
            placeholders.get("plan").map(String::as_str),
            Some("Standard")
        );
    }

    #[test]
    fn percentage_only_plans_skip_the_credit_rows() {
        let now = DateTime::from_timestamp(1_791_014_400, 0).unwrap();
        let snapshot = SyntheticSnapshot {
            plan: None,
            weekly: UsageWindow {
                utilization_pct: 31,
                resets_at: Some(now + chrono::Duration::days(2)),
                window_duration: chrono::Duration::zero(),
            },
            credits: None,
            next_regen_percent: Some(2.0),
            utilization_precise: 30.6,
        };
        let opts = RenderOpts {
            format: None,
            tooltip_format: None,
            icon: None,
            pace_tolerance: 5,
            format_pace_color: false,
            tooltip_pace_pts: false,
        };
        let outcome = VendorOutcome {
            snapshot: crate::usage::VendorSnapshot::Synthetic(snapshot.clone()),
            stale: false,
            last_error: None,
            cache_age: Some(Duration::from_secs(1)),
        };
        let rendered = render(&outcome, &snapshot, &Theme::default(), &opts, now);
        assert!(rendered.tooltip.contains("Next regen +2%"));
        assert!(rendered.tooltip.contains("percentage only"));
        // Without a configured tier the vendor label stands in.
        let placeholders = build_placeholders(&snapshot, &opts, now);
        assert_eq!(
            placeholders.get("plan").map(String::as_str),
            Some("Synthetic")
        );
        assert_eq!(
            placeholders.get("synthetic_requests").map(String::as_str),
            Some("")
        );
        assert_eq!(
            placeholders.get("synthetic_limit").map(String::as_str),
            Some("")
        );
    }
}
