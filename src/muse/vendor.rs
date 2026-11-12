use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::format::{placeholders, substitute, updated_at_hm, window_placeholders};
use crate::pacing::PaceSeverity;
use crate::pango::{color_span, escape, severity_color, severity_for};
use crate::theme::Theme;
use crate::tooltip::{Line, push_window, render_bordered};
use crate::usage::MuseSnapshot;
use crate::vendor::{RenderOpts, VendorId, VendorOutcome};
use crate::waybar::{Class, WaybarOutput};

impl From<super::fetch::FetchOutcome> for VendorOutcome {
    fn from(outcome: super::fetch::FetchOutcome) -> Self {
        outcome.map(crate::usage::VendorSnapshot::MuseCode)
    }
}

pub fn build_placeholders(
    snap: &MuseSnapshot,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> HashMap<&'static str, String> {
    let session = window_placeholders(snap.session.as_ref(), opts, now);
    let weekly = window_placeholders(snap.weekly.as_ref(), opts, now);
    placeholders([
        ("icon", VendorId::MuseCode.bar_icon().into()),
        ("plan", snap.plan.clone()),
        ("vendor_short", VendorId::MuseCode.short_name().into()),
        ("session_pct", session.pct.clone()),
        ("session_reset", session.reset.clone()),
        ("session_elapsed", session.elapsed.clone()),
        ("weekly_pct", weekly.pct.clone()),
        ("weekly_reset", weekly.reset.clone()),
        ("weekly_elapsed", weekly.elapsed.clone()),
        ("muse_plan", snap.plan.clone()),
        ("muse_session_pct", session.pct),
        ("muse_session_reset", session.reset),
        ("muse_session_elapsed", session.elapsed),
        ("muse_session_pace", session.ratio_pace),
        ("muse_session_pace_indicator", session.point_pace),
        ("muse_weekly_pct", weekly.pct),
        ("muse_weekly_reset", weekly.reset),
        ("muse_weekly_elapsed", weekly.elapsed),
        ("muse_weekly_pace", weekly.ratio_pace),
        ("muse_weekly_pace_indicator", weekly.point_pace),
    ])
}

pub fn severity(snap: &MuseSnapshot) -> PaceSeverity {
    severity_for(
        [snap.session.as_ref(), snap.weekly.as_ref()]
            .into_iter()
            .flatten()
            .map(|window| window.utilization_pct)
            .max()
            .unwrap_or(0),
    )
}

pub fn render(
    outcome: &VendorOutcome,
    snap: &MuseSnapshot,
    theme: &Theme,
    opts: &RenderOpts,
    now: DateTime<Utc>,
) -> WaybarOutput {
    let mut values = build_placeholders(snap, opts, now);
    for value in values.values_mut() {
        *value = escape(value);
    }
    let default = match (snap.session.is_some(), snap.weekly.is_some()) {
        (true, true) => "{muse_session_pct}% · {muse_weekly_pct}%w",
        (true, false) => "{muse_session_pct}% · {muse_session_reset}",
        (false, true) => "{muse_weekly_pct}%w · {muse_weekly_reset}",
        _ => "{plan}",
    };
    let mut text = substitute(opts.format.as_deref().unwrap_or(default), &values);
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
            let mut lines = vec![Line::Center(escape(&snap.plan)), Line::Sep];
            if let Some(session) = &snap.session {
                push_window(&mut lines, "Session", session, theme, now, None);
            }
            if let Some(weekly) = &snap.weekly {
                push_window(&mut lines, "Weekly", weekly, theme, now, None);
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
        text: color_span(severity_color(severity(snap), theme), &text),
        tooltip,
        class: Class::from(severity(snap)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::UsageWindow;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_791_014_400, 0).unwrap()
    }

    fn window(pct: i32, duration_hours: i64) -> UsageWindow {
        UsageWindow {
            utilization_pct: pct,
            resets_at: Some(now() + chrono::Duration::hours(2)),
            window_duration: chrono::Duration::hours(duration_hours),
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
    fn placeholders_export_cross_vendor_and_muse_elapsed_and_pace() {
        let snap = MuseSnapshot {
            plan: "Pro".into(),
            session: Some(window(40, 5)),
            weekly: Some(window(70, 168)),
        };
        let values = build_placeholders(&snap, &opts(), now());

        assert_eq!(values.get("plan").map(String::as_str), Some("Pro"));
        assert_eq!(values.get("muse_plan").map(String::as_str), Some("Pro"));
        assert_eq!(values.get("vendor_short").map(String::as_str), Some("msc"));
        assert_eq!(values.get("icon").map(String::as_str), Some("msc"));

        assert_eq!(values.get("session_pct").map(String::as_str), Some("40"));
        assert_eq!(
            values.get("muse_session_pct").map(String::as_str),
            Some("40")
        );
        assert_eq!(
            values.get("session_elapsed"),
            values.get("muse_session_elapsed")
        );
        assert_eq!(values["session_elapsed"], "60");
        assert_eq!(
            values.get("session_reset"),
            values.get("muse_session_reset")
        );
        assert!(!values["session_reset"].is_empty());

        assert_eq!(values.get("weekly_pct").map(String::as_str), Some("70"));
        assert_eq!(
            values.get("muse_weekly_pct").map(String::as_str),
            Some("70")
        );
        assert_eq!(
            values.get("weekly_elapsed"),
            values.get("muse_weekly_elapsed")
        );
        assert_eq!(values["weekly_elapsed"], "98");
        assert_eq!(values.get("weekly_reset"), values.get("muse_weekly_reset"));
        assert!(!values["weekly_reset"].is_empty());

        assert_eq!(values["muse_session_pace"], "↓");
        assert_eq!(values["muse_session_pace_indicator"], "↓");
        assert_eq!(values["muse_weekly_pace"], "↓");
        assert_eq!(values["muse_weekly_pace_indicator"], "↓");
    }

    #[test]
    fn omitted_windows_expand_to_empty_strings() {
        let snap = MuseSnapshot {
            plan: "Pro".into(),
            session: None,
            weekly: None,
        };
        let values = build_placeholders(&snap, &opts(), now());

        assert_eq!(values["session_pct"], "");
        assert_eq!(values["session_reset"], "");
        assert_eq!(values["session_elapsed"], "");
        assert_eq!(values["weekly_pct"], "");
        assert_eq!(values["weekly_reset"], "");
        assert_eq!(values["weekly_elapsed"], "");
        assert_eq!(values["muse_session_pct"], "");
        assert_eq!(values["muse_session_reset"], "");
        assert_eq!(values["muse_session_elapsed"], "");
        assert_eq!(values["muse_session_pace"], "");
        assert_eq!(values["muse_session_pace_indicator"], "");
        assert_eq!(values["muse_weekly_pct"], "");
        assert_eq!(values["muse_weekly_reset"], "");
        assert_eq!(values["muse_weekly_elapsed"], "");
        assert_eq!(values["muse_weekly_pace"], "");
        assert_eq!(values["muse_weekly_pace_indicator"], "");
    }
}
