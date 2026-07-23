//! ShvIA renderer — bar text + bordered Pango tooltip.
//!
//! The single-line widget headlines the WEEK window (ShvIA's primary limit
//! resets weekly). Limited windows render a percentage; unlimited windows
//! (`limit == -1`) render the raw used token count instead.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::countdown;
use crate::format::{placeholders, substitute, updated_at_hm};
use crate::pacing::PaceSeverity;
use crate::pango::{self, color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, render_bordered};
use crate::usage::{ShviaSnapshot, ShviaWindow};
use crate::vendor::{RenderOpts, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};

use super::fetch::FetchOutcome;

pub const DEFAULT_FORMAT: &str = "{shvia_week} · {shvia_week_reset}";

pub fn build_placeholders(
    snap: &ShviaSnapshot,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let today_pct = window_pct(&snap.today);
    let week_pct = window_pct(&snap.week);
    let month_pct = window_pct(&snap.month);
    placeholders(vec![
        ("icon", "󰚩".to_string()),
        ("vendor_short", "shvia".to_string()),
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
        ("shvia_week", window_headline(&snap.week)),
        ("shvia_week_pct", week_pct.to_string()),
        (
            "shvia_week_reset",
            countdown::format(window_reset(&snap.week), now),
        ),
        ("shvia_month", window_headline(&snap.month)),
        ("shvia_month_pct", month_pct.to_string()),
        (
            "shvia_month_reset",
            countdown::format(window_reset(&snap.month), now),
        ),
    ])
}

fn window_reset(w: &Option<ShviaWindow>) -> Option<DateTime<Utc>> {
    w.as_ref().and_then(|w| w.resets_at)
}

fn window_pct(w: &Option<ShviaWindow>) -> i32 {
    w.as_ref().map(|w| w.utilization_pct()).unwrap_or(0)
}

/// The headline string for a window: a percentage when the window has a real
/// limit, the raw used count (compactly formatted) when it is unlimited, or
/// "—" when the window is absent.
fn window_headline(w: &Option<ShviaWindow>) -> String {
    match w.as_ref() {
        None => "—".to_string(),
        Some(w) if w.is_unlimited() => format_count(w.used),
        Some(w) => format!("{}%", w.utilization_pct()),
    }
}

/// Compact integer formatter — `999` stays `999`, `12345` → `12.3k`,
/// `2_400_000` → `2.4M`. Used for unlimited windows where there is no
/// percentage to show, only a raw token count.
fn format_count(n: i64) -> String {
    let abs = n.unsigned_abs();
    if abs >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if abs >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
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
    let values = build_placeholders(snap, now);

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
        render_tooltip(outcome, snap, theme, now)
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
        push_window(&mut lines, "  󰔟  Today", w, theme, now);
    }
    if let Some(w) = snap.week.as_ref() {
        if snap.today.is_some() {
            lines.push(TooltipLine::Body("".into()));
        }
        push_window(&mut lines, "  󰃰  Week", w, theme, now);
    }
    if let Some(w) = snap.month.as_ref() {
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Sep);
        push_window(&mut lines, "  󰓹  Month", w, theme, now);
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

fn push_window(
    lines: &mut Vec<TooltipLine>,
    label: &str,
    w: &ShviaWindow,
    theme: &Theme,
    now: DateTime<Utc>,
) {
    let fg = &theme.fg;
    let dim = &theme.dim;
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{fg}'>{label}</span>"
    )));
    if w.is_unlimited() {
        // No ceiling — show the raw used count, no bar.
        let blue = &theme.blue;
        lines.push(TooltipLine::Body(format!(
            "   <span font_weight='bold' foreground='{blue}'>{used} used</span> \
             <span foreground='{dim}'>· unlimited</span>",
            used = escape(&format_count(w.used))
        )));
    } else {
        let pct = w.utilization_pct();
        let color = severity_color(severity_for(pct), theme);
        let bar = pango::progress_bar(pct, color, theme, None);
        lines.push(TooltipLine::Body(format!(
            "   {bar}  <span font_weight='bold' foreground='{color}'>{pct}%</span>"
        )));
        let remaining = w
            .remaining
            .map(|r| format!(" · {} left", format_count(r)))
            .unwrap_or_default();
        lines.push(TooltipLine::Body(format!(
            "     <span foreground='{dim}'>{used} / {limit}{remaining}</span>",
            used = escape(&format_count(w.used)),
            limit = escape(&format_count(w.limit)),
        )));
    }
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{dim}'>  ⏱  Resets in {cd}</span>",
        cd = escape(&countdown::format(w.resets_at, now))
    )));
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        Self {
            snapshot: crate::usage::VendorSnapshot::Shvia(o.snapshot),
            stale: o.stale,
            last_error: o.last_error,
            cache_age: o.cache_age,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{ShviaSnapshot, ShviaWindow};

    fn sample_snap() -> ShviaSnapshot {
        let now = Utc::now();
        ShviaSnapshot {
            plan: "ShvIA".into(),
            today: Some(ShviaWindow {
                used: 1234,
                limit: 100_000,
                remaining: Some(98_766),
                resets_at: Some(now + chrono::Duration::hours(8)),
            }),
            week: Some(ShviaWindow {
                used: 250_000,
                limit: 500_000,
                remaining: Some(250_000),
                resets_at: Some(now + chrono::Duration::days(3)),
            }),
            month: Some(ShviaWindow {
                used: 40_000,
                limit: -1,
                remaining: None,
                resets_at: Some(now + chrono::Duration::days(9)),
            }),
        }
    }

    fn outcome(s: ShviaSnapshot) -> VendorOutcome {
        VendorOutcome {
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
                used: 12_345,
                limit: -1,
                remaining: None,
                resets_at: None,
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

    #[test]
    fn custom_tooltip_uses_placeholders() {
        let snap = sample_snap();
        let oc = outcome(snap.clone());
        let mut o = opts();
        o.tooltip_format = Some("T:{shvia_today_pct} W:{shvia_week_pct}".into());
        let out = render(&oc, &snap, &Theme::default(), &o, Utc::now());
        assert_eq!(out.tooltip, "T:1 W:50");
    }

    #[test]
    fn format_count_scales() {
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(12_345), "12.3k");
        assert_eq!(format_count(2_400_000), "2.4M");
    }
}
