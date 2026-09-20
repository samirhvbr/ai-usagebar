//! ShvIA renderer — bar text + bordered Pango tooltip.
//!
//! The single-line widget headlines the WEEK window (ShvIA's primary limit
//! resets weekly). Limited windows render a percentage and go through the
//! shared [`push_window_with_row`]; unlimited windows (`limit == -1`) have no
//! ratio to draw, so they render the raw used token count instead.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::countdown;
use crate::format::{compact_count, placeholders, substitute, updated_at_hm};
use crate::pacing::{self, PaceSeverity};
use crate::pango::{color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, WindowRow, push_window_with_row, render_bordered};
use crate::usage::{ShviaSnapshot, ShviaWindow};
use crate::vendor::{RenderOpts, VendorId, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};

use super::fetch::FetchOutcome;

pub const DEFAULT_FORMAT: &str = "{shvia_week} · {shvia_week_reset}";

/// Build placeholders with the historical default pacing tolerance.
///
/// Keep this signature stable for library callers. Rendering uses the private
/// tolerance-aware helper so `--pace-tolerance` still applies.
pub fn build_placeholders(
    snap: &ShviaSnapshot,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    build_placeholders_with_tolerance(snap, pacing::DEFAULT_TOLERANCE, now)
}

fn build_placeholders_with_tolerance(
    snap: &ShviaSnapshot,
    pace_tolerance: u32,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let today_pct = window_pct(&snap.today);
    let week_pct = window_pct(&snap.week);
    let month_pct = window_pct(&snap.month);
    let today = window_pacing(snap.today.as_ref(), pace_tolerance, now);
    let week = window_pacing(snap.week.as_ref(), pace_tolerance, now);
    let month = window_pacing(snap.month.as_ref(), pace_tolerance, now);
    let mut values = vec![
        ("icon", "󰚩".to_string()),
        ("vendor_short", VendorId::Shvia.short_name().to_string()),
        // Cross-vendor aliases for scroll-cycle friendly formats. ShvIA's
        // weekly window is the headline, so it maps to the generic
        // `{session_*}` / `{weekly_*}` aliases other vendors expose.
        ("session_pct", week_pct.to_string()),
        (
            "session_reset",
            countdown::format(window_reset(&snap.week), now),
        ),
        ("weekly_pct", week_pct.to_string()),
        (
            "weekly_reset",
            countdown::format(window_reset(&snap.week), now),
        ),
        ("session_elapsed", week.elapsed.clone()),
        ("weekly_elapsed", week.elapsed.clone()),
        ("plan", snap.plan.clone()),
        ("shvia_plan", snap.plan.clone()),
        // Per-window headline strings ("43%" for limited, "12.3k" for
        // unlimited) plus the raw pct + reset countdowns.
        ("shvia_today", window_headline(&snap.today)),
        ("shvia_today_pct", today_pct.to_string()),
        (
            "shvia_today_reset",
            countdown::format(window_reset(&snap.today), now),
        ),
        ("shvia_today_elapsed", today.elapsed),
        ("shvia_today_pace", today.ratio_pace),
        ("shvia_today_pace_indicator", today.point_pace),
        ("shvia_week", window_headline(&snap.week)),
        ("shvia_week_pct", week_pct.to_string()),
        (
            "shvia_week_reset",
            countdown::format(window_reset(&snap.week), now),
        ),
        ("shvia_week_elapsed", week.elapsed),
        ("shvia_week_pace", week.ratio_pace),
        ("shvia_week_pace_indicator", week.point_pace),
        ("shvia_month", window_headline(&snap.month)),
        ("shvia_month_pct", month_pct.to_string()),
        (
            "shvia_month_reset",
            countdown::format(window_reset(&snap.month), now),
        ),
        ("shvia_month_elapsed", month.elapsed),
        ("shvia_month_pace", month.ratio_pace),
        ("shvia_month_pace_indicator", month.point_pace),
    ];
    // The raw counters the bar deliberately does not spell out beside a
    // percentage it already draws — the same call upstream made for Kimi.
    // Anyone who wants the figures puts them in a custom `--format`.
    for (window, name) in [
        (&snap.today, "today"),
        (&snap.week, "week"),
        (&snap.month, "month"),
    ] {
        let (used, limit, remaining) = counters(window);
        values.push((counter_key(name, "used"), used));
        values.push((counter_key(name, "limit"), limit));
        values.push((counter_key(name, "remaining"), remaining));
    }
    placeholders(values)
}

/// The placeholder names for a window's raw counters. `&'static str` keys, so
/// the three-window loop above cannot invent one at runtime.
fn counter_key(window: &str, field: &str) -> &'static str {
    match (window, field) {
        ("today", "used") => "shvia_today_used",
        ("today", "limit") => "shvia_today_limit",
        ("today", "remaining") => "shvia_today_remaining",
        ("week", "used") => "shvia_week_used",
        ("week", "limit") => "shvia_week_limit",
        ("week", "remaining") => "shvia_week_remaining",
        ("month", "used") => "shvia_month_used",
        ("month", "limit") => "shvia_month_limit",
        ("month", "remaining") => "shvia_month_remaining",
        _ => unreachable!("unknown ShvIA counter placeholder: {window}_{field}"),
    }
}

/// `(used, limit, remaining)` as display strings. An absent window is empty
/// rather than `0`, so a format that names it does not claim a figure the
/// gateway never reported; an unlimited window's limit reads `unlimited`.
fn counters(w: &Option<ShviaWindow>) -> (String, String, String) {
    let Some(w) = w.as_ref() else {
        return (String::new(), String::new(), String::new());
    };
    (
        w.used.to_string(),
        if w.is_unlimited() {
            "unlimited".to_string()
        } else {
            w.limit.to_string()
        },
        w.remaining.map(|r| r.to_string()).unwrap_or_default(),
    )
}

fn window_reset(w: &Option<ShviaWindow>) -> Option<DateTime<Utc>> {
    w.as_ref().and_then(|w| w.resets_at)
}

fn window_pct(w: &Option<ShviaWindow>) -> i32 {
    w.as_ref().map(|w| w.utilization_pct()).unwrap_or(0)
}

/// Elapsed fraction and pace glyphs for one window, as ready placeholder
/// values. Empty for an absent window, and for an unlimited one — pacing
/// compares consumption against elapsed time, and there is nothing to consume
/// against when the window has no ceiling.
#[derive(Default)]
struct WindowPacing {
    elapsed: String,
    ratio_pace: String,
    point_pace: String,
}

fn window_pacing(w: Option<&ShviaWindow>, pace_tolerance: u32, now: DateTime<Utc>) -> WindowPacing {
    let Some(w) = w.and_then(ShviaWindow::as_usage_window) else {
        return WindowPacing::default();
    };
    let p = pacing::calc(
        w.utilization_pct,
        w.resets_at,
        now,
        w.window_duration,
        pace_tolerance,
    );
    WindowPacing {
        elapsed: p.elapsed_pct.to_string(),
        ratio_pace: p.ratio_pace.glyph().to_string(),
        point_pace: p.point_pace.glyph().to_string(),
    }
}

/// The headline string for a window: a percentage when the window has a real
/// limit, the raw used count (compactly formatted) when it is unlimited, or
/// "—" when the window is absent.
fn window_headline(w: &Option<ShviaWindow>) -> String {
    match w.as_ref() {
        None => "—".to_string(),
        Some(w) if w.is_unlimited() => compact_count(w.used),
        Some(w) => format!("{}%", w.utilization_pct()),
    }
}

pub fn severity(snap: &ShviaSnapshot) -> PaceSeverity {
    let worst = [
        window_pct(&snap.today),
        window_pct(&snap.week),
        window_pct(&snap.month),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    severity_for(worst)
}

pub fn render(
    outcome: &VendorOutcome,
    snap: &ShviaSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let class = Class::from(severity(snap));
    let format = opts
        .format
        .clone()
        .unwrap_or_else(|| DEFAULT_FORMAT.to_string());
    let values = build_placeholders_with_tolerance(snap, opts.pace_tolerance, now);

    let mut text = substitute(&format, &values);
    if outcome.stale {
        text.push_str(" ⏸");
    }
    let wrapper_color = severity_color(severity(snap), theme).to_string();
    let icon_prefix = match opts.icon.as_deref() {
        Some(ic) if !ic.is_empty() => format!("{ic} "),
        _ => String::new(),
    };
    let bar_text = color_span(&wrapper_color, &format!("{icon_prefix}{text}"));

    let tooltip = if let Some(fmt) = opts.tooltip_format.as_deref() {
        substitute(fmt, &values)
    } else {
        render_tooltip(outcome, snap, theme, opts, now)
    };

    WaybarOutput {
        text: bar_text,
        tooltip,
        class,
    }
}

fn render_tooltip(
    outcome: &VendorOutcome,
    snap: &ShviaSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> String {
    let blue = &theme.blue;
    let dim = &theme.dim;
    let mut lines: Vec<TooltipLine> = Vec::new();
    lines.push(TooltipLine::Center(format!(
        "<span font_weight='bold' foreground='{blue}'>{plan}</span>",
        plan = escape(&snap.plan)
    )));
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body("".into()));

    if let Some(w) = snap.today.as_ref() {
        push_shvia_window(&mut lines, "  󰔟  Today", w, theme, opts, now);
    }
    if let Some(w) = snap.week.as_ref() {
        if snap.today.is_some() {
            lines.push(TooltipLine::Body("".into()));
        }
        push_shvia_window(&mut lines, "  󰃰  Week", w, theme, opts, now);
    }
    if let Some(w) = snap.month.as_ref() {
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Sep);
        push_shvia_window(&mut lines, "  󰓹  Month", w, theme, opts, now);
    }
    if snap.today.is_none() && snap.week.is_none() && snap.month.is_none() {
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>no usage windows reported</span>"
        )));
    }

    if let Some((code, msg)) = outcome.last_error.as_ref()
        && *code != 0
    {
        let (icon, ecolor) = if *code >= 500 {
            ("󰅚", theme.red.as_str())
        } else {
            ("󰀪", theme.orange.as_str())
        };
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Sep);
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{ecolor}'>  {icon}  HTTP {code}</span>"
        )));
        lines.push(TooltipLine::Body(format!(
            "     <span foreground='{dim}'>{}</span>",
            escape(msg)
        )));
    }

    let updated = updated_at_hm(now, outcome.cache_age);
    lines.push(TooltipLine::Body("".into()));
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{dim}'>  󰅐  Updated {updated}</span>"
    )));

    render_bordered(&lines, theme)
}

/// One window's rows. A limited window is an ordinary quota row and goes
/// through the shared renderer, so it looks like every other vendor's; an
/// unlimited one cannot — there is no ratio — and says how much was used and
/// that nothing caps it.
fn push_shvia_window(
    lines: &mut Vec<TooltipLine>,
    label: &str,
    w: &ShviaWindow,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) {
    if let Some(uw) = w.as_usage_window() {
        let row = WindowRow::paced(&uw, now, opts.pace_tolerance, opts.tooltip_pace_pts);
        push_window_with_row(lines, label, &uw, theme, now, row);
        return;
    }

    let fg = &theme.fg;
    let dim = &theme.dim;
    let blue = &theme.blue;
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{fg}'>{label}</span>"
    )));
    lines.push(TooltipLine::Body(format!(
        "   <span font_weight='bold' foreground='{blue}'>{used} used</span> \
         <span foreground='{dim}'>· unlimited</span>",
        used = escape(&compact_count(w.used))
    )));
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{dim}'>  ⏱  Resets in {cd}</span>",
        cd = escape(&countdown::format(w.resets_at, now))
    )));
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Shvia)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{ShviaSnapshot, ShviaWindow};

    fn window(used: i64, limit: i64, remaining: Option<i64>, in_days: i64) -> ShviaWindow {
        ShviaWindow {
            used,
            limit,
            remaining,
            resets_at: Some(Utc::now() + chrono::Duration::days(in_days)),
            window_duration: chrono::Duration::days(in_days.max(1)),
        }
    }

    fn sample_snap() -> ShviaSnapshot {
        ShviaSnapshot {
            plan: "ShvIA".into(),
            today: Some(ShviaWindow {
                resets_at: Some(Utc::now() + chrono::Duration::hours(8)),
                window_duration: chrono::Duration::days(1),
                ..window(1234, 100_000, Some(98_766), 1)
            }),
            week: Some(window(250_000, 500_000, Some(250_000), 3)),
            month: Some(window(40_000, -1, None, 9)),
        }
    }

    fn outcome(s: ShviaSnapshot) -> VendorOutcome {
        crate::outcome::Outcome {
            snapshot: crate::usage::VendorSnapshot::Shvia(s),
            stale: false,
            last_error: None,
            cache_age: Some(std::time::Duration::from_secs(10)),
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
    fn default_format_headlines_week() {
        let snap = sample_snap();
        let oc = outcome(snap.clone());
        let out = render(&oc, &snap, &Theme::default(), &opts(), Utc::now());
        // week is 250000/500000 = 50%.
        assert!(out.text.contains("50%"));
    }

    #[test]
    fn tooltip_contains_all_windows_present() {
        let snap = sample_snap();
        let oc = outcome(snap.clone());
        let out = render(&oc, &snap, &Theme::default(), &opts(), Utc::now());
        assert!(out.tooltip.contains("Today"));
        assert!(out.tooltip.contains("Week"));
        assert!(out.tooltip.contains("Month"));
        // Month is unlimited → shows used + "unlimited".
        assert!(out.tooltip.contains("unlimited"));
    }

    #[test]
    fn unlimited_window_headline_shows_count_not_pct() {
        let snap = ShviaSnapshot {
            plan: "ShvIA".into(),
            today: None,
            week: Some(ShviaWindow {
                resets_at: None,
                ..window(12_345, -1, None, 7)
            }),
            month: None,
        };
        let oc = outcome(snap.clone());
        let mut o = opts();
        o.format = Some("{shvia_week}".into());
        let out = render(&oc, &snap, &Theme::default(), &o, Utc::now());
        // 12345 → "12.3k", and no "%" in the headline.
        assert!(out.text.contains("12.3k"));
        assert!(!out.text.contains('%'));
    }

    #[test]
    fn empty_snapshot_renders_no_windows_message() {
        let snap = ShviaSnapshot {
            plan: "ShvIA".into(),
            today: None,
            week: None,
            month: None,
        };
        let oc = outcome(snap.clone());
        let out = render(&oc, &snap, &Theme::default(), &opts(), Utc::now());
        assert!(out.tooltip.contains("no usage windows reported"));
    }

    #[test]
    fn severity_picks_worst_window() {
        let mut snap = sample_snap();
        // Drive today to 95% → critical.
        snap.today.as_mut().unwrap().used = 95_000;
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }

    /// An unlimited window reports 0% by construction, which must not drag the
    /// vendor's severity down from what the limited windows say.
    #[test]
    fn unlimited_window_does_not_mask_a_hot_one() {
        let snap = ShviaSnapshot {
            plan: "ShvIA".into(),
            today: None,
            week: Some(window(95, 100, Some(5), 3)),
            month: Some(window(10_000_000, -1, None, 20)),
        };
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }

    #[test]
    fn custom_tooltip_uses_placeholders() {
        let snap = sample_snap();
        let oc = outcome(snap.clone());
        let mut o = opts();
        o.tooltip_format = Some("T:{shvia_today_pct} W:{shvia_week_pct}".into());
        let out = render(&oc, &snap, &Theme::default(), &o, Utc::now());
        assert_eq!(out.tooltip, "T:1 W:50");
    }

    /// The counters left the tooltip when the rows moved to the shared
    /// renderer, so the placeholders are now the only way to get them.
    #[test]
    fn raw_counters_are_available_as_placeholders() {
        let snap = sample_snap();
        let values = build_placeholders(&snap, Utc::now());
        assert_eq!(values.get("shvia_week_used").unwrap(), "250000");
        assert_eq!(values.get("shvia_week_limit").unwrap(), "500000");
        assert_eq!(values.get("shvia_week_remaining").unwrap(), "250000");
        // Unlimited: a ceiling that does not exist is named, not printed as -1.
        assert_eq!(values.get("shvia_month_limit").unwrap(), "unlimited");
        assert_eq!(values.get("shvia_month_remaining").unwrap(), "");
        // Absent window: empty, not a fabricated zero.
        let empty = ShviaSnapshot {
            plan: "ShvIA".into(),
            today: None,
            week: None,
            month: None,
        };
        let values = build_placeholders(&empty, Utc::now());
        assert_eq!(values.get("shvia_today_used").unwrap(), "");
    }
}
