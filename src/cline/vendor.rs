use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::countdown;
use crate::format::{placeholders, substitute, updated_at_hm};
use crate::pacing::PaceSeverity;
use crate::pango::{color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, render_bordered};
use crate::vendor::{RenderOpts, VendorId, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};

use super::fetch::FetchOutcome;
use super::types::{Snapshot, Window};

pub const DEFAULT_FORMAT: &str = "{cline_5h_pct}% · {cline_5h_reset}";
const DEFAULT_PLAN: &str = "Cline";
const UNAVAILABLE: &str = "—";

impl From<FetchOutcome> for VendorOutcome {
    fn from(outcome: FetchOutcome) -> Self {
        outcome.map(crate::usage::VendorSnapshot::Cline)
    }
}

pub fn build_placeholders(snap: &Snapshot, now: DateTime<Utc>) -> HashMap<&'static str, String> {
    let plan = sanitize(snap.plan.as_deref().unwrap_or(DEFAULT_PLAN));
    let five = window_values(snap.five_hour.as_ref(), now);
    let seven = window_values(snap.seven_day.as_ref(), now);
    let thirty = window_values(snap.thirty_day.as_ref(), now);

    placeholders([
        ("vendor_short", VendorId::Cline.short_name().to_string()),
        ("plan", plan.clone()),
        ("cline_plan", plan),
        // The 5-hour window is the one that throttles a session, so it fills
        // the generic session placeholders; the 7-day one fills the weekly
        // pair every other subscription vendor exposes.
        ("session_pct", five.percent.clone()),
        ("session_reset", five.reset.clone()),
        ("weekly_pct", seven.percent.clone()),
        ("weekly_reset", seven.reset.clone()),
        ("cline_5h_pct", five.percent),
        ("cline_5h_reset", five.reset),
        ("cline_7d_pct", seven.percent),
        ("cline_7d_reset", seven.reset),
        ("cline_30d_pct", thirty.percent),
        ("cline_30d_reset", thirty.reset),
        (
            "cline_period_end",
            snap.period_end
                .map(|at| at.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| UNAVAILABLE.to_string()),
        ),
    ])
}

#[derive(Debug)]
struct WindowValues {
    percent: String,
    reset: String,
}

fn window_values(window: Option<&Window>, now: DateTime<Utc>) -> WindowValues {
    let Some(window) = window else {
        return WindowValues {
            percent: UNAVAILABLE.to_string(),
            reset: UNAVAILABLE.to_string(),
        };
    };
    WindowValues {
        percent: window.pct().to_string(),
        // A window holding nothing has no charge waiting to age out, so there
        // is no countdown to show — not a countdown to zero.
        reset: window
            .resets_at()
            .map(|at| countdown::format(Some(at), now))
            .unwrap_or_else(|| UNAVAILABLE.to_string()),
    }
}

fn sanitize(value: &str) -> String {
    crate::display::sanitize_untrusted_field(value)
}

pub fn severity(snap: &Snapshot) -> PaceSeverity {
    snap.worst_pct()
        .map(severity_for)
        .unwrap_or(PaceSeverity::Low)
}

pub fn render(
    outcome: &VendorOutcome,
    snap: &Snapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    render_with_meta(
        snap,
        outcome.stale,
        outcome.last_error.as_ref(),
        outcome.cache_age,
        theme,
        opts,
        now,
    )
}

fn render_with_meta(
    snap: &Snapshot,
    stale: bool,
    last_error: Option<&(u16, String)>,
    cache_age: Option<Duration>,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let sev = severity(snap);
    let format = opts.format.as_deref().unwrap_or(DEFAULT_FORMAT);
    let values = escaped_placeholders(snap, now);
    let mut text = substitute(format, &values);
    if stale {
        text.push_str(" ⏸");
    }
    let icon_prefix = match opts.icon.as_deref() {
        Some(icon) if !icon.is_empty() => format!("{} ", escape(icon)),
        _ => String::new(),
    };
    let bar_text = color_span(severity_color(sev, theme), &format!("{icon_prefix}{text}"));
    let tooltip = opts
        .tooltip_format
        .as_deref()
        .map(|format| substitute(format, &values))
        .unwrap_or_else(|| render_tooltip(snap, stale, last_error, cache_age, theme, now));

    WaybarOutput {
        text: bar_text,
        tooltip,
        class: Class::from(sev),
    }
}

fn escaped_placeholders(snap: &Snapshot, now: DateTime<Utc>) -> HashMap<&'static str, String> {
    let mut values = build_placeholders(snap, now);
    for key in ["plan", "cline_plan"] {
        if let Some(value) = values.get_mut(key) {
            *value = escape(value);
        }
    }
    values
}

fn render_tooltip(
    snap: &Snapshot,
    stale: bool,
    last_error: Option<&(u16, String)>,
    cache_age: Option<Duration>,
    theme: &Theme,
    now: DateTime<Utc>,
) -> String {
    let mut lines = vec![TooltipLine::Center(format!(
        "<span font_weight='bold' foreground='{}'>{}</span>",
        theme.blue,
        escape(&sanitize(snap.plan.as_deref().unwrap_or(DEFAULT_PLAN)))
    ))];
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body(String::new()));

    let mut present = false;
    for (label, window) in [
        ("5 hours ", snap.five_hour.as_ref()),
        ("Weekly  ", snap.seven_day.as_ref()),
        ("Monthly ", snap.thirty_day.as_ref()),
    ] {
        let Some(window) = window else {
            continue;
        };
        present = true;
        let values = window_values(Some(window), now);
        lines.push(TooltipLine::Body(format!(
            "  {}  {}% · {}",
            label,
            escape(&values.percent),
            escape(&values.reset)
        )));
    }
    if !present {
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{}'>this plan has no ClinePass quota</span>",
            theme.dim
        )));
    }
    if stale {
        lines.push(TooltipLine::Body(String::new()));
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{}'>  ⏸  Showing cached data</span>",
            theme.orange
        )));
    }
    if let Some((code, message)) = last_error
        && *code != 0
    {
        lines.push(TooltipLine::Body(String::new()));
        lines.push(TooltipLine::Sep);
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{}'>  HTTP {code}: {}</span>",
            theme.orange,
            escape(message)
        )));
    }

    lines.push(TooltipLine::Body(String::new()));
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{}'>  Updated {}</span>",
        theme.dim,
        updated_at_hm(now, cache_age)
    )));
    render_bordered(&lines, theme)
}

#[cfg(test)]
mod tests {

    use super::*;

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().expect("RFC3339 timestamp")
    }

    fn window(percent: f64, reset: Option<&str>) -> Window {
        Window {
            percent,
            reset: reset.map(at),
        }
    }

    fn sample() -> Snapshot {
        Snapshot {
            plan: Some("Cline Pass (Monthly)".into()),
            five_hour: Some(window(10.0, Some("2026-09-09T22:00:00Z"))),
            seven_day: Some(window(50.0, Some("2026-09-12T17:00:00Z"))),
            thirty_day: Some(window(10.0, Some("2026-09-19T17:00:00Z"))),
            period_end: Some(at("2026-10-01T05:17:51Z")),
        }
    }

    #[test]
    fn exposes_exact_cline_and_generic_placeholders() {
        let values = build_placeholders(&sample(), at("2026-09-09T20:00:00Z"));

        assert_eq!(values["vendor_short"], "cln");
        assert_eq!(values["cline_5h_pct"], "10");
        assert_eq!(values["cline_7d_pct"], "50");
        assert_eq!(values["cline_30d_pct"], "10");
        assert_eq!(values["session_pct"], "10");
        assert_eq!(values["weekly_pct"], "50");
        assert_eq!(values["cline_plan"], "Cline Pass (Monthly)");
        assert_eq!(values["cline_period_end"], "2026-10-01");
    }

    #[test]
    fn absent_windows_are_unavailable_not_zero() {
        let values = build_placeholders(
            &Snapshot {
                plan: None,
                five_hour: None,
                seven_day: None,
                thirty_day: None,
                period_end: None,
            },
            at("2026-09-09T20:00:00Z"),
        );

        for key in [
            "cline_5h_pct",
            "cline_7d_pct",
            "cline_30d_pct",
            "session_pct",
            "weekly_pct",
            "cline_5h_reset",
            "cline_period_end",
        ] {
            assert_eq!(values[key], "—", "{key} should be unavailable");
        }
        assert_eq!(values["cline_plan"], "Cline");
    }

    #[test]
    fn an_idle_window_shows_no_countdown() {
        let mut snapshot = sample();
        snapshot.five_hour = Some(window(0.0, None));
        let values = build_placeholders(&snapshot, at("2026-09-09T20:00:00Z"));

        assert_eq!(values["cline_5h_pct"], "0");
        assert_eq!(values["cline_5h_reset"], "—");
    }

    #[test]
    fn severity_follows_the_most_exhausted_window() {
        let mut snapshot = sample();
        snapshot.thirty_day = Some(window(98.0, None));
        assert_eq!(severity(&snapshot), severity_for(98));
    }

    #[test]
    fn plan_names_from_the_api_are_sanitized() {
        let mut snapshot = sample();
        snapshot.plan = Some("Cline\u{1b}[31m Pass\u{7}".into());
        let values = build_placeholders(&snapshot, at("2026-09-09T20:00:00Z"));

        assert!(!values["cline_plan"].contains('\u{1b}'));
        assert!(!values["cline_plan"].contains('\u{7}'));
    }

    #[test]
    fn default_format_leads_with_the_five_hour_window() {
        assert_eq!(DEFAULT_FORMAT, "{cline_5h_pct}% · {cline_5h_reset}");
    }
}
