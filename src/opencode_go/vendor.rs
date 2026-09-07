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
use super::types::{Usage, Window};

pub const DEFAULT_FORMAT: &str = "{ocg_rolling_pct}% · {ocg_rolling_reset}";
const DEFAULT_PLAN: &str = "OpenCode Go";
const UNAVAILABLE: &str = "—";

impl From<FetchOutcome> for VendorOutcome {
    fn from(outcome: FetchOutcome) -> Self {
        outcome.map(crate::usage::VendorSnapshot::OpenCodeGo)
    }
}

pub fn build_placeholders(usage: &Usage, now: DateTime<Utc>) -> HashMap<&'static str, String> {
    build_placeholders_with_plan(DEFAULT_PLAN, usage, now)
}

pub fn build_placeholders_with_plan(
    plan: &str,
    usage: &Usage,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let plan = sanitize(plan);
    let rolling = window_values(usage.rolling.as_ref(), now);
    let weekly = window_values(usage.weekly.as_ref(), now);
    let monthly = window_values(usage.monthly.as_ref(), now);

    placeholders([
        (
            "vendor_short",
            VendorId::OpenCodeGo.short_name().to_string(),
        ),
        ("plan", plan.clone()),
        ("ocg_plan", plan),
        ("session_pct", rolling.percent.clone()),
        ("session_reset", rolling.reset.clone()),
        ("weekly_pct", weekly.percent.clone()),
        ("weekly_reset", weekly.reset.clone()),
        ("ocg_rolling_pct", rolling.percent),
        ("ocg_rolling_reset", rolling.reset),
        ("ocg_rolling_status", rolling.status),
        ("ocg_weekly_pct", weekly.percent),
        ("ocg_weekly_reset", weekly.reset),
        ("ocg_weekly_status", weekly.status),
        ("ocg_monthly_pct", monthly.percent),
        ("ocg_monthly_reset", monthly.reset),
        ("ocg_monthly_status", monthly.status),
    ])
}

#[derive(Debug)]
struct WindowValues {
    percent: String,
    reset: String,
    status: String,
}

fn window_values(window: Option<&Window>, now: DateTime<Utc>) -> WindowValues {
    let Some(window) = window else {
        return WindowValues {
            percent: UNAVAILABLE.to_string(),
            reset: UNAVAILABLE.to_string(),
            status: UNAVAILABLE.to_string(),
        };
    };
    WindowValues {
        percent: window.percent.to_string(),
        reset: countdown::format(Some(window.resets_at), now),
        status: sanitize(&window.status),
    }
}

fn sanitize(value: &str) -> String {
    crate::display::sanitize_untrusted_field(value)
}

pub fn severity(usage: &Usage) -> PaceSeverity {
    usage
        .rolling
        .iter()
        .chain(usage.weekly.iter())
        .chain(usage.monthly.iter())
        .map(|window| window.percent as i32)
        .max()
        .map(severity_for)
        .unwrap_or(PaceSeverity::Low)
}

/// Renderer shape matches the existing vendor adapters. `snap` remains local
/// because the shared enum does not yet have an OpenCode-Go arm.
pub fn render(
    outcome: &VendorOutcome,
    snap: &Usage,
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
    snap: &Usage,
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

fn escaped_placeholders(usage: &Usage, now: DateTime<Utc>) -> HashMap<&'static str, String> {
    let mut values = build_placeholders(usage, now);
    for key in [
        "plan",
        "ocg_plan",
        "ocg_rolling_status",
        "ocg_weekly_status",
        "ocg_monthly_status",
    ] {
        if let Some(value) = values.get_mut(key) {
            *value = escape(value);
        }
    }
    values
}

fn render_tooltip(
    snap: &Usage,
    stale: bool,
    last_error: Option<&(u16, String)>,
    cache_age: Option<Duration>,
    theme: &Theme,
    now: DateTime<Utc>,
) -> String {
    let mut lines = vec![TooltipLine::Center(format!(
        "<span font_weight='bold' foreground='{}'>{}</span>",
        theme.blue,
        escape(DEFAULT_PLAN)
    ))];
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body(String::new()));

    let mut present = false;
    for (label, window) in [
        ("Rolling", snap.rolling.as_ref()),
        ("Weekly", snap.weekly.as_ref()),
        ("Monthly", snap.monthly.as_ref()),
    ] {
        let Some(window) = window else {
            continue;
        };
        present = true;
        let values = window_values(Some(window), now);
        lines.push(TooltipLine::Body(format!(
            "  {}  {}% · {} · {}",
            label,
            escape(&values.percent),
            escape(&values.reset),
            escape(&values.status)
        )));
    }
    if !present {
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{}'>no usage windows reported</span>",
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
    use chrono::{DateTime, Utc};

    use super::*;
    use crate::opencode_go::types::{Usage, Window};

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().expect("RFC3339 timestamp")
    }

    fn sample_usage() -> Usage {
        Usage {
            rolling: Some(Window {
                status: "ok".into(),
                percent: 12.3,
                resets_at: at("2026-08-16T20:00:00Z"),
            }),
            weekly: Some(Window {
                status: "rate-limited".into(),
                percent: 45.6,
                resets_at: at("2026-08-20T00:00:00Z"),
            }),
            monthly: Some(Window {
                status: "ok".into(),
                percent: 78.9,
                resets_at: at("2026-09-01T00:00:00Z"),
            }),
        }
    }

    #[test]
    fn exposes_exact_opencode_go_and_generic_placeholders() {
        let values = build_placeholders(&sample_usage(), at("2026-08-16T18:00:00Z"));

        assert_eq!(values["vendor_short"], "ocg");
        assert_eq!(values["session_pct"], "12.3");
        assert_eq!(values["weekly_pct"], "45.6");
        assert_eq!(values["ocg_rolling_pct"], "12.3");
        assert_eq!(values["ocg_rolling_status"], "ok");
        assert_eq!(values["ocg_weekly_pct"], "45.6");
        assert_eq!(values["ocg_weekly_status"], "rate-limited");
        assert_eq!(values["ocg_monthly_pct"], "78.9");
        assert_eq!(values["ocg_monthly_status"], "ok");
    }

    #[test]
    fn default_format_is_rolling_percentage_and_reset() {
        assert_eq!(DEFAULT_FORMAT, "{ocg_rolling_pct}% · {ocg_rolling_reset}");
    }

    #[test]
    fn absent_windows_are_unavailable_not_zero() {
        let values = build_placeholders(
            &Usage {
                rolling: None,
                weekly: None,
                monthly: None,
            },
            at("2026-08-16T18:00:00Z"),
        );

        for key in [
            "session_pct",
            "weekly_pct",
            "ocg_rolling_pct",
            "ocg_weekly_pct",
            "ocg_monthly_pct",
        ] {
            assert_eq!(values[key], "—", "{key} should be unavailable");
            assert_ne!(values[key], "0");
        }
        for key in [
            "session_reset",
            "weekly_reset",
            "ocg_rolling_reset",
            "ocg_weekly_reset",
            "ocg_monthly_reset",
            "ocg_rolling_status",
            "ocg_weekly_status",
            "ocg_monthly_status",
        ] {
            assert_eq!(values[key], "—", "{key} should be unavailable");
        }
    }

    #[test]
    fn plan_and_status_are_sanitized() {
        let usage = Usage {
            rolling: Some(Window {
                status: "ok\u{1b}[31m\u{7}".into(),
                percent: 1.0,
                resets_at: at("2026-08-16T20:00:00Z"),
            }),
            weekly: None,
            monthly: None,
        };
        let values = build_placeholders_with_plan(
            "OpenCode\u{1b}[31m Go",
            &usage,
            at("2026-08-16T18:00:00Z"),
        );

        assert!(!values["plan"].contains('\u{1b}'));
        assert!(!values["ocg_rolling_status"].contains('\u{1b}'));
        assert!(!values["ocg_rolling_status"].contains('\u{7}'));
    }
}
