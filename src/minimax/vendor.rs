//! MiniMax renderer — bar text + bordered Pango tooltip.
//!
//! MiniMax reports quota per model bucket, so its rows are labeled by pool the
//! same way Antigravity labels its Gemini / third-party groups. The text pool
//! is what the bar shows; the video pool, when the plan has one, appears in the
//! tooltip rather than competing for space on the bar.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::countdown;
use crate::format::{placeholders, substitute, updated_at_hm};
use crate::pacing::PaceSeverity;
use crate::pango::{color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, render_bordered};
use crate::usage::{MinimaxSnapshot, UsageWindow};
use crate::vendor::{RenderOpts, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};

use super::fetch::FetchOutcome;

/// The text & coding pool (`general` on the wire) — what the bars represent.
pub const POOL_GENERAL: &str = "Text";
/// The video-generation pool (`video` on the wire), shown when the plan has it.
pub const POOL_VIDEO: &str = "Video";

pub const DEFAULT_FORMAT: &str = "{minimax_session_pct}% · {minimax_session_reset}";

pub fn build_placeholders(
    snap: &MinimaxSnapshot,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let session_pct = snap.session.utilization_pct;
    let weekly_pct = snap.weekly.utilization_pct;
    let session_reset = countdown::format(snap.session.resets_at, now);
    let weekly_reset = countdown::format(snap.weekly.resets_at, now);
    // The video pool is optional; its placeholders resolve to an em dash rather
    // than vanishing, so a user format referencing them never leaves a gap.
    let video = |w: &Option<UsageWindow>, pct: bool| -> String {
        match w {
            Some(w) if pct => w.utilization_pct.to_string(),
            Some(w) => countdown::format(w.resets_at, now),
            None => "—".to_string(),
        }
    };

    placeholders(vec![
        ("icon", "󰚩".to_string()),
        ("vendor_short", "mmx".to_string()),
        // Cross-vendor aliases — what the desktop surfaces read.
        ("plan", snap.plan.clone()),
        ("session_pct", session_pct.to_string()),
        ("session_reset", session_reset.clone()),
        ("weekly_pct", weekly_pct.to_string()),
        ("weekly_reset", weekly_reset.clone()),
        // MiniMax-specific placeholders.
        ("minimax_plan", snap.plan.clone()),
        ("minimax_session_pct", session_pct.to_string()),
        ("minimax_session_reset", session_reset),
        ("minimax_weekly_pct", weekly_pct.to_string()),
        ("minimax_weekly_reset", weekly_reset),
        ("minimax_video_pct", video(&snap.video_session, true)),
        ("minimax_video_reset", video(&snap.video_session, false)),
        ("minimax_video_weekly_pct", video(&snap.video_weekly, true)),
    ])
}

/// Worst of the two text-pool windows. The video pool deliberately does not
/// drive the bar color: running out of video quota should not paint the coding
/// bar red.
pub fn severity(snap: &MinimaxSnapshot) -> PaceSeverity {
    severity_for(
        snap.session
            .utilization_pct
            .max(snap.weekly.utilization_pct),
    )
}

pub fn render(
    outcome: &VendorOutcome,
    snap: &MinimaxSnapshot,
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
    // User formats are Pango markup after Waybar renders them. Escape API
    // strings there, while retaining raw values for the default tooltip (which
    // escapes exactly once at its markup insertion point).
    let mut pango_values = values.clone();
    for key in ["plan", "minimax_plan"] {
        if let Some(value) = pango_values.get_mut(key) {
            *value = escape(value);
        }
    }

    let mut text = substitute(&format, &pango_values);
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
        substitute(fmt, &pango_values)
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
    snap: &MinimaxSnapshot,
    theme: &Theme,
    now: DateTime<Utc>,
) -> String {
    let blue = &theme.blue;
    let dim = &theme.dim;
    let fg = &theme.fg;

    let mut lines: Vec<TooltipLine> = Vec::new();
    lines.push(TooltipLine::Center(format!(
        "<span font_weight='bold' foreground='{blue}'>{}</span>",
        escape(&snap.plan)
    )));
    lines.push(TooltipLine::Sep);

    let mut pool = |label: &str, icon: &str, session: &UsageWindow, weekly: &UsageWindow| {
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{fg}'>  {icon}  {label}</span>"
        )));
        for (what, w) in [("Session", session), ("Weekly", weekly)] {
            let color = severity_color(severity_for(w.utilization_pct), theme);
            lines.push(TooltipLine::Body(format!(
                "   <span foreground='{dim}'>{what}</span>  \
                 <span font_weight='bold' foreground='{color}'>{pct}%</span>",
                pct = w.utilization_pct
            )));
            lines.push(TooltipLine::Body(format!(
                " <span foreground='{dim}'>     reset {}</span>",
                escape(&countdown::format(w.resets_at, now))
            )));
        }
    };

    pool(POOL_GENERAL, "󰅄", &snap.session, &snap.weekly);
    if let (Some(vs), Some(vw)) = (&snap.video_session, &snap.video_weekly) {
        pool(POOL_VIDEO, "󰕧", vs, vw);
    }

    if let Some((code, msg)) = outcome.last_error.as_ref() {
        let (label, icon, ecolor) = if *code == 0 {
            ("MiniMax error".to_string(), "󰅚", theme.red.as_str())
        } else if *code >= 500 {
            (format!("HTTP {code}"), "󰅚", theme.red.as_str())
        } else {
            (format!("HTTP {code}"), "󰀪", theme.orange.as_str())
        };
        lines.push(TooltipLine::Body("".into()));
        lines.push(TooltipLine::Sep);
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{ecolor}'>  {icon}  {label}</span>"
        )));
        if msg != &label {
            lines.push(TooltipLine::Body(format!(
                "     <span foreground='{dim}'>{}</span>",
                escape(msg)
            )));
        }
    }

    let updated = updated_at_hm(now, outcome.cache_age);
    lines.push(TooltipLine::Body("".into()));
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{dim}'>  󰅐  Updated {updated}</span>"
    )));

    render_bordered(&lines, theme)
}

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        Self {
            snapshot: crate::usage::VendorSnapshot::Minimax(o.snapshot),
            stale: o.stale,
            last_error: o.last_error,
            cache_age: o.cache_age,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 27, 12, 0, 0).unwrap()
    }

    fn window(pct: i32, mins_ahead: i64, dur: chrono::Duration) -> UsageWindow {
        UsageWindow {
            utilization_pct: pct,
            resets_at: Some(now() + chrono::Duration::minutes(mins_ahead)),
            window_duration: dur,
        }
    }

    fn snap() -> MinimaxSnapshot {
        MinimaxSnapshot {
            plan: "MiniMax Token Plan".to_string(),
            session: window(31, 45, chrono::Duration::hours(5)),
            weekly: window(62, 3000, chrono::Duration::days(7)),
            video_session: Some(window(5, 200, chrono::Duration::hours(24))),
            video_weekly: Some(window(9, 3000, chrono::Duration::days(7))),
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

    fn outcome(s: &MinimaxSnapshot) -> VendorOutcome {
        VendorOutcome {
            snapshot: crate::usage::VendorSnapshot::Minimax(s.clone()),
            stale: false,
            last_error: None,
            cache_age: Some(std::time::Duration::ZERO),
        }
    }

    #[test]
    fn default_format_shows_session_percent_and_reset() {
        let s = snap();
        let out = render(&outcome(&s), &s, &Theme::default(), &opts(), now());
        assert!(out.text.contains("31%"), "bar text was {:?}", out.text);
    }

    /// Cross-vendor aliases are what the GNOME/macOS surfaces read; without
    /// them MiniMax would render blank rows on the desktop.
    #[test]
    fn exposes_cross_vendor_aliases() {
        let v = build_placeholders(&snap(), now());
        assert_eq!(v.get("session_pct").map(String::as_str), Some("31"));
        assert_eq!(v.get("weekly_pct").map(String::as_str), Some("62"));
        assert_eq!(v.get("vendor_short").map(String::as_str), Some("mmx"));
        assert!(v.contains_key("session_reset"));
    }

    /// The video pool must not drag the coding bar into red.
    #[test]
    fn severity_ignores_the_video_pool() {
        let mut s = snap();
        s.session.utilization_pct = 10;
        s.weekly.utilization_pct = 10;
        s.video_session = Some(window(99, 10, chrono::Duration::hours(24)));
        s.video_weekly = Some(window(99, 10, chrono::Duration::days(7)));
        assert_eq!(severity(&s), severity_for(10));
    }

    #[test]
    fn video_placeholders_degrade_to_a_dash_without_the_pool() {
        let mut s = snap();
        s.video_session = None;
        s.video_weekly = None;
        let v = build_placeholders(&s, now());
        assert_eq!(v.get("minimax_video_pct").map(String::as_str), Some("—"));
        assert_eq!(
            v.get("minimax_video_weekly_pct").map(String::as_str),
            Some("—")
        );
    }

    #[test]
    fn tooltip_lists_both_pools_when_present() {
        let s = snap();
        let tip = render_tooltip(&outcome(&s), &s, &Theme::default(), now());
        assert!(tip.contains(POOL_GENERAL));
        assert!(tip.contains(POOL_VIDEO));
    }

    #[test]
    fn tooltip_omits_the_video_pool_when_absent() {
        let mut s = snap();
        s.video_session = None;
        s.video_weekly = None;
        let tip = render_tooltip(&outcome(&s), &s, &Theme::default(), now());
        assert!(tip.contains(POOL_GENERAL));
        assert!(!tip.contains(POOL_VIDEO));
    }

    /// Plan text reaches Pango exactly once escaped, from both paths.
    #[test]
    fn escapes_the_plan_name_exactly_once() {
        let mut s = snap();
        s.plan = "Plan <b>&</b>".to_string();
        let out = render(
            &outcome(&s),
            &s,
            &Theme::default(),
            &RenderOpts {
                format: Some("{minimax_plan}".to_string()),
                ..opts()
            },
            now(),
        );
        assert!(out.text.contains("&lt;b&gt;&amp;&lt;/b&gt;"), "{:?}", out.text);
        assert!(!out.text.contains("&amp;lt;"), "double-escaped: {:?}", out.text);
    }

    #[test]
    fn stale_marks_the_bar() {
        let s = snap();
        let mut o = outcome(&s);
        o.stale = true;
        let out = render(&o, &s, &Theme::default(), &opts(), now());
        assert!(out.text.contains('⏸'));
    }
}
