//! Cursor renderer — bar text + bordered Pango tooltip. Two included-usage
//! pools (Cursor Models / Other Models), like Kimi's two windows but keyed on
//! model category rather than a time window.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::countdown;
use crate::display::sanitize_untrusted_line;
use crate::format::{
    cursor_credit_label, cursor_credit_line, cursor_credit_meter, placeholders, substitute,
    updated_at_hm,
};
use crate::pacing::{self, PaceSeverity, Pacing};
use crate::pango::{color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line as TooltipLine, WindowRow, push_window_with_row, render_bordered};
use crate::usage::{CursorSnapshot, UsageWindow};
use crate::vendor::{RenderOpts, VendorId, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};

use super::fetch::FetchOutcome;

pub const DEFAULT_FORMAT: &str = "{cursor_auto_pct}·{cursor_api_pct}%";

/// No confirmed Cursor glyph in the common Nerd Font sets, so a plain caret.
/// Override with `--icon`.
const DEFAULT_ICON: &str = "❯";

/// Placeholder names of one pool's pace family, in the order
/// [`pace_entries`] fills them: ratio glyph, ratio label, point glyph, point
/// label, signed point delta.
const AUTO_PACE_KEYS: [&str; 5] = [
    "cursor_auto_pace",
    "cursor_auto_pace_pct",
    "cursor_auto_pace_indicator",
    "cursor_auto_pace_pts",
    "cursor_auto_pace_delta",
];
const API_PACE_KEYS: [&str; 5] = [
    "cursor_api_pace",
    "cursor_api_pace_pct",
    "cursor_api_pace_indicator",
    "cursor_api_pace_pts",
    "cursor_api_pace_delta",
];

/// Whether the elapsed share of the billing cycle is knowable. An unlimited
/// plan has no cap to pace against, and a cycle whose start the API did not
/// send has no exact length — [`CursorSnapshot::cycle_window`] never guesses
/// a month, so neither does the pace.
fn has_cycle_pace(snap: &CursorSnapshot) -> bool {
    !snap.unlimited && snap.cycle_window().is_some()
}

/// Pace of one pool against the billing cycle. Both pools share the cycle, so
/// only the usage differs. A cycle of unknown length is neutral.
fn pool_pacing(snap: &CursorSnapshot, pct: i32, tolerance: u32, now: DateTime<Utc>) -> Pacing {
    match snap.cycle_window() {
        Some(window) if !snap.unlimited => pacing::calc(pct, snap.reset_at, now, window, tolerance),
        _ => Pacing::neutral(),
    }
}

fn pace_entries(
    keys: [&'static str; 5],
    pace: Pacing,
    unlimited: bool,
) -> [(&'static str, String); 5] {
    // An unlimited plan resolves to empty strings (the missing-placeholder
    // convention) rather than a neutral pace for a cap that does not exist.
    let shown = |value: String| if unlimited { String::new() } else { value };
    [
        (keys[0], shown(pace.ratio_pace.glyph().into())),
        (keys[1], shown(pace.ratio_label)),
        (keys[2], shown(pace.point_pace.glyph().into())),
        (keys[3], shown(pace.point_label)),
        (keys[4], shown(pace.delta.to_string())),
    ]
}

pub fn build_placeholders(
    snap: &CursorSnapshot,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    build_placeholders_with_tolerance(snap, pacing::DEFAULT_TOLERANCE, now)
}

fn build_placeholders_with_tolerance(
    snap: &CursorSnapshot,
    tolerance: u32,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let reset = countdown::format(snap.reset_at, now);
    let auto_pace = pool_pacing(snap, snap.auto_pct, tolerance, now);
    let elapsed = if has_cycle_pace(snap) {
        auto_pace.elapsed_pct.to_string()
    } else {
        String::new()
    };
    let api_pace = pool_pacing(snap, snap.api_pct, tolerance, now);
    let mut pairs = vec![
        ("icon", DEFAULT_ICON.to_string()),
        ("vendor_short", VendorId::Cursor.short_name().to_string()),
        // Cross-vendor aliases: the two pools map onto the two generic windows
        // (session = Cursor Models, weekly = Other Models) so a shared format
        // and the macOS menu bar show both. Severity still keys on the worst.
        ("plan", format!("Cursor {}", snap.plan)),
        ("session_pct", snap.auto_pct.to_string()),
        ("session_reset", reset.clone()),
        // Both pools reset with the billing cycle, so one elapsed share serves
        // both aliases — it is what places the macOS pace marker.
        ("session_elapsed", elapsed.clone()),
        ("weekly_pct", snap.api_pct.to_string()),
        ("weekly_reset", reset.clone()),
        ("weekly_elapsed", elapsed.clone()),
        // Cursor-specific placeholders.
        ("cursor_plan", snap.plan.clone()),
        ("cursor_auto_pct", snap.auto_pct.to_string()),
        ("cursor_api_pct", snap.api_pct.to_string()),
        ("cursor_total_pct", snap.total_pct.to_string()),
        ("cursor_reset", reset),
        ("cursor_elapsed", elapsed),
        (
            "cursor_on_demand",
            if snap.on_demand_enabled { "on" } else { "off" }.to_string(),
        ),
        (
            "cursor_unlimited",
            if snap.unlimited { "yes" } else { "no" }.to_string(),
        ),
        ("cursor_credits", credit_placeholder(snap, now)),
    ];
    pairs.extend(pace_entries(AUTO_PACE_KEYS, auto_pace, snap.unlimited));
    pairs.extend(pace_entries(API_PACE_KEYS, api_pace, snap.unlimited));
    placeholders(pairs)
}

/// One grant per segment. Empty when the account has no visible grant, so a
/// format that includes the placeholder stays quiet.
fn credit_placeholder(snap: &CursorSnapshot, now: DateTime<Utc>) -> String {
    snap.credits
        .iter()
        .map(|grant| {
            let line = cursor_credit_line(
                grant.remaining_cents,
                grant.total_cents,
                grant.expires_at,
                now,
            );
            // The bar wraps this string in Pango. A grant name is free text, so
            // drop the characters that would close that markup. The tooltip
            // escapes them instead and still shows the name.
            let name: String =
                cursor_credit_label(&sanitize_untrusted_line(grant.display_name.trim()))
                    .chars()
                    .filter(|ch| !matches!(ch, '<' | '>' | '&'))
                    .collect();
            format!("{name} · {line}")
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Severity keys on the binding pool. An unlimited plan has no cap, so it stays
/// calm regardless of the (meaningless) percentages.
pub fn severity(snap: &CursorSnapshot) -> PaceSeverity {
    if snap.unlimited {
        PaceSeverity::Low
    } else {
        severity_for(snap.worst_pct())
    }
}

pub fn render(
    outcome: &VendorOutcome,
    snap: &CursorSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let class = Class::from(severity(snap));
    let format = opts
        .format
        .clone()
        .unwrap_or_else(|| DEFAULT_FORMAT.to_string());
    let mut values = build_placeholders_with_tolerance(snap, opts.pace_tolerance, now);
    // Both sinks fed by this map (bar text and --tooltip-format) are Pango
    // markup. The plan label is API-controlled, so escape its aliases at the
    // projection boundary. The default tooltip escapes the raw snapshot.
    for key in ["plan", "cursor_plan"] {
        if let Some(value) = values.get_mut(key) {
            *value = escape(value);
        }
    }
    let pango_values = pace_colored(&values, snap, theme, opts, now);

    let mut text = if snap.unlimited && opts.format.is_none() {
        "unlimited".to_string()
    } else {
        substitute(&format, &pango_values)
    };
    if outcome.stale {
        text.push_str(" ⏸");
    }

    let wrapper_color = if opts.format_pace_color && format.contains("_pace") {
        theme.fg.clone()
    } else {
        severity_color(severity(snap), theme).to_string()
    };
    let icon_prefix = match opts.icon.as_deref() {
        Some(ic) if !ic.is_empty() => format!("{ic} "),
        _ => String::new(),
    };
    let bar_text = color_span(&wrapper_color, &format!("{icon_prefix}{text}"));

    let tooltip = if let Some(fmt) = opts.tooltip_format.as_deref() {
        substitute(fmt, &pango_values)
    } else {
        render_tooltip(outcome, snap, theme, opts, now)
    };

    WaybarOutput {
        text: bar_text,
        tooltip,
        class,
    }
}

/// `--format-pace-color`: wrap every pace placeholder in the colour of its
/// own point delta, so each pool is coloured by its own pace.
fn pace_colored(
    values: &HashMap<&'static str, String>,
    snap: &CursorSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let mut colored = values.clone();
    if !opts.format_pace_color || snap.unlimited {
        return colored;
    }
    for (keys, pct) in [
        (AUTO_PACE_KEYS, snap.auto_pct),
        (API_PACE_KEYS, snap.api_pct),
    ] {
        let pace = pool_pacing(snap, pct, opts.pace_tolerance, now);
        let color = severity_color(pacing::pace_severity(pace.delta), theme);
        for key in keys {
            if let Some(value) = colored.get_mut(key) {
                *value = color_span(color, value);
            }
        }
    }
    colored
}

/// One pool as the shared gauge block: label, bar with percentage and pace
/// glyph, reset countdown, then a dim description of what the pool covers.
///
/// Both pools share the billing cycle, so the window is the cycle itself. A
/// cycle of unknown length gets a bar with no glyph and no marker: no pace
/// beats a fabricated `→` (see [`has_cycle_pace`]).
fn push_pool(
    lines: &mut Vec<TooltipLine>,
    snap: &CursorSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
    pool: PoolRow<'_>,
) {
    let window = UsageWindow {
        utilization_pct: pool.pct,
        resets_at: snap.reset_at,
        window_duration: snap.cycle_window().unwrap_or_else(chrono::Duration::zero),
    };
    let row = if has_cycle_pace(snap) {
        WindowRow::paced(&window, now, opts.pace_tolerance, opts.tooltip_pace_pts)
    } else {
        WindowRow::default()
    };
    push_window_with_row(lines, pool.label, &window, theme, now, row);
    let dim = &theme.dim;
    lines.push(TooltipLine::Body(format!(
        " <span foreground='{dim}'>     {}</span>",
        escape(&pool.description)
    )));
}

struct PoolRow<'a> {
    label: &'a str,
    pct: i32,
    description: String,
}

fn render_tooltip(
    outcome: &VendorOutcome,
    snap: &CursorSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> String {
    let blue = &theme.blue;
    let dim = &theme.dim;
    let fg = &theme.fg;

    let mut lines: Vec<TooltipLine> = Vec::new();
    lines.push(TooltipLine::Center(format!(
        "<span font_weight='bold' foreground='{blue}'>Cursor {}</span>",
        escape(&snap.plan)
    )));
    lines.push(TooltipLine::Sep);
    lines.push(TooltipLine::Body("".into()));

    if snap.unlimited {
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{fg}'>  󰐾  Unlimited plan</span>"
        )));
    } else {
        let auto = PoolRow {
            label: "  󰢻  Cursor Models",
            pct: snap.auto_pct,
            description: "Auto + Composer".into(),
        };
        push_pool(&mut lines, snap, theme, opts, now, auto);
        lines.push(TooltipLine::Body("".into()));
        let api = PoolRow {
            label: "  󰢻  Other Models",
            pct: snap.api_pct,
            description: format!(
                "Named / API models · on-demand {}",
                if snap.on_demand_enabled { "on" } else { "off" }
            ),
        };
        push_pool(&mut lines, snap, theme, opts, now, api);
    }

    for grant in &snap.credits {
        let label = cursor_credit_label(&sanitize_untrusted_line(grant.display_name.trim()));
        let (pct, _, _) = cursor_credit_meter(
            grant.remaining_cents,
            grant.total_cents,
            grant.expires_at,
            now,
        );
        let line = cursor_credit_line(
            grant.remaining_cents,
            grant.total_cents,
            grant.expires_at,
            now,
        );
        // Same gauge block as the two pools, but paced against nothing: a
        // grant has its own expiry, not the billing cycle. The label is
        // API-controlled, so it is escaped before entering the Pango sink.
        let window = UsageWindow {
            utilization_pct: i32::from(pct),
            resets_at: grant.expires_at,
            window_duration: chrono::Duration::zero(),
        };
        lines.push(TooltipLine::Body("".into()));
        push_window_with_row(
            &mut lines,
            &escape(&label),
            &window,
            theme,
            now,
            WindowRow::default(),
        );
        lines.push(TooltipLine::Body(format!(
            " <span foreground='{dim}'>     {}</span>",
            escape(&line)
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

impl From<FetchOutcome> for VendorOutcome {
    fn from(o: FetchOutcome) -> Self {
        o.map(crate::usage::VendorSnapshot::Cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 26, 12, 0, 0).unwrap()
    }

    fn sample_snap() -> CursorSnapshot {
        CursorSnapshot {
            plan: "Ultra".into(),
            auto_pct: 98,
            api_pct: 100,
            total_pct: 99,
            unlimited: false,
            on_demand_enabled: false,
            on_demand_used_cents: None,
            on_demand_limit_cents: None,
            reset_at: Some(now() + chrono::Duration::days(9)),
            cycle_start: None,
            credits: Vec::new(),
        }
    }

    fn sample_outcome(snap: CursorSnapshot) -> VendorOutcome {
        VendorOutcome {
            snapshot: crate::usage::VendorSnapshot::Cursor(snap),
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
    fn default_bar_shows_both_pools() {
        let snap = sample_snap();
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(out.text.contains("98·100%"), "text: {}", out.text);
    }

    #[test]
    fn tooltip_breaks_out_both_pools_and_reset() {
        let snap = sample_snap();
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(out.tooltip.contains("Cursor Ultra"));
        assert!(out.tooltip.contains("Cursor Models"));
        assert!(out.tooltip.contains("98%"));
        assert!(out.tooltip.contains("Other Models"));
        assert!(out.tooltip.contains("100%"));
        assert!(out.tooltip.contains("9d"));
        assert!(out.tooltip.contains('█'), "each pool is drawn as a bar");
        assert!(!out.tooltip.contains("Credits"), "no grant, no credit row");
    }

    #[test]
    fn tooltip_and_placeholder_show_the_spending_page_grant() {
        let mut snap = sample_snap();
        let expires = now() + chrono::Duration::days(30);
        snap.credits.push(crate::usage::CursorCreditGrant {
            remaining_cents: 2100,
            total_cents: 2500,
            expires_at: Some(expires),
            display_name: "Promo\n<script>".into(),
        });
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(out.text.contains("98·100%"), "the bar stays the two pools");
        assert!(out.tooltip.contains("Credits"));
        assert!(!out.tooltip.contains("Promo"));
        assert!(out.tooltip.contains("16%"));
        assert!(out.tooltip.contains("$21.00/$25.00 remaining"));
        assert!(out.tooltip.contains("30d 0h"));
        assert!(!out.tooltip.contains("<script>"));
        let values = build_placeholders(&snap, now());
        assert!(values["cursor_credits"].starts_with("Credits "));
        assert!(values["cursor_credits"].contains("$21.00/$25.00 remaining"));
        assert!(values["cursor_credits"].contains("30d 0h"));
    }

    #[test]
    fn severity_keys_on_the_worst_pool() {
        let mut snap = sample_snap();
        snap.auto_pct = 10;
        snap.api_pct = 95;
        // Other Models at 95% must drive severity Critical even though Cursor
        // Models is calm.
        assert_eq!(severity(&snap), PaceSeverity::Critical);
    }

    #[test]
    fn unlimited_plan_is_calm_and_labeled() {
        let mut snap = sample_snap();
        snap.unlimited = true;
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert_eq!(severity(&snap), PaceSeverity::Low);
        assert!(out.text.contains("unlimited"));
        assert!(out.tooltip.contains("Unlimited plan"));
    }

    #[test]
    fn stale_appends_pause() {
        let snap = sample_snap();
        let mut outcome = sample_outcome(snap.clone());
        outcome.stale = true;
        let out = render(&outcome, &snap, &Theme::default(), &opts(), now());
        assert!(out.text.contains("⏸"));
    }

    #[test]
    fn custom_tooltip_uses_placeholders() {
        let snap = sample_snap();
        let mut o = opts();
        o.tooltip_format = Some("auto {cursor_auto_pct} api {cursor_api_pct} {cursor_plan}".into());
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &o,
            now(),
        );
        assert_eq!(out.tooltip, "auto 98 api 100 Ultra");
    }

    #[test]
    fn api_plan_is_pango_escaped_in_custom_formats() {
        let mut snap = sample_snap();
        snap.plan = "Pro & Ultra <beta>".into();
        let outcome = sample_outcome(snap.clone());
        let mut o = opts();
        o.format = Some("{plan}".into());
        o.tooltip_format = Some("{cursor_plan}".into());

        let out = render(&outcome, &snap, &Theme::default(), &o, now());
        assert!(!out.text.contains(" & "));
        assert!(!out.tooltip.contains('<'));
        assert!(out.text.contains("Cursor Pro &amp; Ultra &lt;beta&gt;"));
        assert_eq!(out.tooltip, "Pro &amp; Ultra &lt;beta&gt;");
    }

    #[test]
    fn generic_windows_map_to_the_two_pools() {
        let values = build_placeholders(&sample_snap(), now());
        assert_eq!(values["session_pct"], "98"); // Cursor Models (auto)
        assert_eq!(values["weekly_pct"], "100"); // Other Models (api)
        assert_eq!(values["plan"], "Cursor Ultra");
    }

    #[test]
    fn placeholder_set_contains_all_keys() {
        let values = build_placeholders(&sample_snap(), now());
        for key in [
            "icon",
            "vendor_short",
            "plan",
            "session_pct",
            "session_reset",
            "weekly_pct",
            "weekly_reset",
            "cursor_plan",
            "cursor_auto_pct",
            "cursor_api_pct",
            "cursor_total_pct",
            "cursor_reset",
            "cursor_on_demand",
            "cursor_unlimited",
            "cursor_credits",
            "session_elapsed",
            "weekly_elapsed",
            "cursor_elapsed",
        ]
        .into_iter()
        .chain(AUTO_PACE_KEYS)
        .chain(API_PACE_KEYS)
        {
            assert!(values.contains_key(key), "missing placeholder {key}");
        }
    }

    /// A 10-day cycle, half gone: Cursor Models at 70% runs ahead, Other
    /// Models at 30% runs behind.
    fn paced_snap() -> CursorSnapshot {
        CursorSnapshot {
            auto_pct: 70,
            api_pct: 30,
            total_pct: 50,
            reset_at: Some(now() + chrono::Duration::days(5)),
            cycle_start: Some(now() - chrono::Duration::days(5)),
            ..sample_snap()
        }
    }

    #[test]
    fn each_pool_is_paced_against_the_billing_cycle() {
        let values = build_placeholders(&paced_snap(), now());
        for key in ["session_elapsed", "weekly_elapsed", "cursor_elapsed"] {
            assert_eq!(values[key], "50", "{key}");
        }
        assert_eq!(values["cursor_auto_pace"], "↑");
        assert_eq!(values["cursor_auto_pace_indicator"], "↑");
        assert_eq!(values["cursor_auto_pace_pct"], "40% ahead");
        assert_eq!(values["cursor_auto_pace_pts"], "20pts ahead");
        assert_eq!(values["cursor_auto_pace_delta"], "20");
        assert_eq!(values["cursor_api_pace"], "↓");
        assert_eq!(values["cursor_api_pace_indicator"], "↓");
        assert_eq!(values["cursor_api_pace_pct"], "40% under");
        assert_eq!(values["cursor_api_pace_pts"], "20pts under");
        assert_eq!(values["cursor_api_pace_delta"], "-20");
    }

    #[test]
    fn pace_follows_the_tolerance_the_widget_was_given() {
        let snap = paced_snap();
        let outcome = sample_outcome(snap.clone());
        let mut o = opts();
        o.format = Some("{cursor_auto_pace} {cursor_auto_pace_indicator}".into());
        let strict = render(&outcome, &snap, &Theme::default(), &o, now());
        assert!(strict.text.contains("↑ ↑"), "{}", strict.text);
        // 70% against 50% elapsed is a 140% ratio: inside a 50-point band the
        // ratio glyph calms down while the point glyph still reports the gap.
        o.pace_tolerance = 50;
        let tolerant = render(&outcome, &snap, &Theme::default(), &o, now());
        assert!(tolerant.text.contains("→ ↑"), "{}", tolerant.text);
    }

    #[test]
    fn pace_color_paints_each_pool_by_its_own_delta() {
        let snap = paced_snap();
        let theme = Theme::default();
        let mut o = opts();
        o.format = Some("{cursor_auto_pace}{cursor_api_pace}".into());
        o.format_pace_color = true;
        let out = render(&sample_outcome(snap.clone()), &snap, &theme, &o, now());
        assert!(
            out.text.contains(&format!("foreground='{}'>↑", theme.red)),
            "{}",
            out.text
        );
        assert!(
            out.text
                .contains(&format!("foreground='{}'>↓", theme.green)),
            "{}",
            out.text
        );
        // The pace colours own the text, so the severity wrapper steps aside.
        assert!(
            !out.text
                .starts_with(&format!("<span foreground='{}'", theme.red)),
            "{}",
            out.text
        );
    }

    #[test]
    fn tooltip_draws_a_bar_per_pool_with_its_pace_glyph() {
        let snap = paced_snap();
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(out.tooltip.contains("70% ↑"), "{}", out.tooltip);
        assert!(out.tooltip.contains("30% ↓"), "{}", out.tooltip);
        assert_eq!(
            out.tooltip.matches("Resets in").count(),
            2,
            "{}",
            out.tooltip
        );
        assert!(out.tooltip.contains("Auto + Composer"), "{}", out.tooltip);
        assert!(out.tooltip.contains("on-demand off"), "{}", out.tooltip);
    }

    #[test]
    fn tooltip_point_mode_uses_the_point_glyph_and_draws_the_elapsed_marker() {
        let snap = paced_snap();
        let outcome = sample_outcome(snap.clone());
        let mut o = opts();
        o.pace_tolerance = 50;
        let ratio = render(&outcome, &snap, &Theme::default(), &o, now());
        // 70% against 50% elapsed is a 140% ratio: inside a 50-point band the
        // ratio glyph calms down.
        assert!(ratio.tooltip.contains("70% →"), "{}", ratio.tooltip);
        o.tooltip_pace_pts = true;
        let points = render(&outcome, &snap, &Theme::default(), &o, now());
        // Point mode ignores the tolerance and adds the marker inside the bar.
        assert!(points.tooltip.contains("70% ↑"), "{}", points.tooltip);
        assert_ne!(ratio.tooltip, points.tooltip, "the marker must be drawn");
    }

    #[test]
    fn a_pool_past_its_allowance_still_draws_a_bar() {
        let snap = CursorSnapshot {
            auto_pct: 143,
            ..paced_snap()
        };
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(out.tooltip.contains("143%"), "{}", out.tooltip);
    }

    #[test]
    fn an_unstated_billing_cycle_is_never_paced() {
        // `billingCycleStart` missing (an old response, or a cache written
        // before it was stored): the cycle length is unknown, and a guessed
        // month would put a pace on the bar as if it were exact.
        let snap = CursorSnapshot {
            cycle_start: None,
            ..paced_snap()
        };
        let values = build_placeholders(&snap, now());
        for key in ["session_elapsed", "weekly_elapsed", "cursor_elapsed"] {
            assert_eq!(values[key], "", "{key}");
        }
        assert_eq!(values["cursor_auto_pace"], "→");
        assert_eq!(values["cursor_auto_pace_pts"], "on track");
        assert_eq!(values["cursor_api_pace_delta"], "0");

        let outcome = sample_outcome(snap.clone());
        let plain = render(&outcome, &snap, &Theme::default(), &opts(), now());
        let mut o = opts();
        o.tooltip_pace_pts = true;
        let points = render(&outcome, &snap, &Theme::default(), &o, now());
        assert_eq!(plain.tooltip, points.tooltip, "no marker without a window");
        assert!(!plain.tooltip.contains('→'), "{}", plain.tooltip);
    }

    #[test]
    fn an_unordered_billing_cycle_is_never_paced() {
        let snap = CursorSnapshot {
            cycle_start: Some(now() + chrono::Duration::days(6)),
            ..paced_snap()
        };
        assert_eq!(build_placeholders(&snap, now())["cursor_elapsed"], "");
    }

    #[test]
    fn an_unlimited_plan_has_no_pace_to_report() {
        let snap = CursorSnapshot {
            unlimited: true,
            ..paced_snap()
        };
        let values = build_placeholders(&snap, now());
        for key in ["session_elapsed", "cursor_elapsed"]
            .into_iter()
            .chain(AUTO_PACE_KEYS)
            .chain(API_PACE_KEYS)
        {
            assert_eq!(values[key], "", "{key} must render empty");
        }
        let out = render(
            &sample_outcome(snap.clone()),
            &snap,
            &Theme::default(),
            &opts(),
            now(),
        );
        assert!(!out.tooltip.contains('↑'), "{}", out.tooltip);
    }

    #[test]
    fn fetch_outcome_conversion_preserves_metadata() {
        let fetch = FetchOutcome {
            snapshot: sample_snap(),
            stale: true,
            last_error: Some((401, "bad".into())),
            cache_age: Some(std::time::Duration::from_secs(42)),
        };
        let vendor: VendorOutcome = fetch.into();
        assert!(matches!(
            vendor.snapshot,
            crate::usage::VendorSnapshot::Cursor(_)
        ));
        assert!(vendor.stale);
    }
}
