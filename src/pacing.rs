//! Pacing math — encodes claudebar's `calc_pacing` (claudebar:279-321) and
//! `pace_color_for` (claudebar:212-219) as pure functions.
//!
//! Two parallel notions of "pacing":
//! - **Ratio** — `actual_pct / elapsed_pct`, with a tolerance band (`PACE_TOLERANCE`).
//!   Used for the `{*_pace}` and `{*_pace_pct}` placeholders. Capped at 999%.
//! - **Point delta** — `actual_pct - elapsed_pct`, a signed integer.
//!   Used for `{*_pace_indicator}`, `{*_pace_pts}`, `{*_pace_delta}`. No tolerance.
//!
//! Both are computed in one shot and returned as a `Pacing` struct so the
//! caller can pick whichever placeholder it needs without re-running the math.

use chrono::{DateTime, Utc};

/// Default tolerance band (in percentage points) for the ratio-based pacing
/// icon. Mirrors claudebar's default `PACE_TOLERANCE=5`.
pub const DEFAULT_TOLERANCE: u32 = 5;

/// A small enum captures the three visual pace states. Keeping the icon out
/// of strings lets the TUI render `Style`-colored chars and the widget render
/// raw glyphs without any string parsing on the other end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    Ahead,
    OnTrack,
    Under,
}

impl Pace {
    /// Single-char glyph used in claudebar's `{*_pace*}` placeholders.
    pub fn glyph(self) -> &'static str {
        match self {
            Pace::Ahead => "↑",
            Pace::OnTrack => "→",
            Pace::Under => "↓",
        }
    }
}

/// Result of `calc_pacing` — all fields the caller might want to render.
///
/// Field naming mirrors the placeholders so the format-substitution layer is
/// a trivial mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pacing {
    /// `{*_elapsed}` — integer percent of the window that has elapsed (0..=100).
    pub elapsed_pct: i32,
    /// `{*_pace}` — ratio-based icon, honors `tolerance`.
    pub ratio_pace: Pace,
    /// `{*_pace_indicator}` — point-based icon, no tolerance.
    pub point_pace: Pace,
    /// `{*_pace_delta}` — signed integer `usage_pct - elapsed_pct`.
    pub delta: i32,
    /// `{*_pace_pct}` — ratio-based label ("12% ahead" / "5% under" / "on track").
    pub ratio_label: String,
    /// `{*_pace_pts}` — point-based label ("12pts ahead" / "5pts under" / "on track").
    pub point_label: String,
}

impl Pacing {
    /// The `{*_elapsed}` field value: the integer meta position when the window
    /// has a known reset, or an empty string when it doesn't. Desktop surfaces
    /// (GNOME/macOS/Windows) key off "is this a number?" to decide whether to
    /// draw the meta marker at all.
    pub fn elapsed_field(&self, has_reset: bool) -> String {
        if has_reset {
            self.elapsed_pct.to_string()
        } else {
            String::new()
        }
    }

    /// Neutral pacing for windows with no `resets_at` (e.g. vendors that don't
    /// expose one). Matches claudebar's early-return value.
    pub fn neutral() -> Self {
        Self {
            elapsed_pct: 0,
            ratio_pace: Pace::OnTrack,
            point_pace: Pace::OnTrack,
            delta: 0,
            ratio_label: "on track".into(),
            point_label: "on track".into(),
        }
    }
}

/// Compute pacing for a usage window.
///
/// `usage_pct` is the vendor-reported utilization (0..=100, integer to match
/// Claude's `utilization` field). `reset` is when the window rolls over;
/// `now` is the reference time (passed in for testability). `window` is the
/// window's total duration. `tolerance` is the ratio-tolerance band in
/// percentage points (e.g. `5` for ±5%).
pub fn calc(
    usage_pct: i32,
    reset: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    window: chrono::Duration,
    tolerance: u32,
) -> Pacing {
    let Some(reset) = reset else {
        return Pacing::neutral();
    };
    if window.num_seconds() <= 0 {
        return Pacing::neutral();
    }

    let remaining = reset.signed_duration_since(now).num_seconds();
    let total = window.num_seconds();
    let mut elapsed_pct = (((total - remaining) * 100) / total) as i32;
    elapsed_pct = elapsed_pct.clamp(0, 100);

    // Point-based delta and label.
    let delta = usage_pct - elapsed_pct;
    let (point_pace, point_label) = if delta > 0 {
        (Pace::Ahead, format!("{delta}pts ahead"))
    } else if delta < 0 {
        (Pace::Under, format!("{}pts under", -delta))
    } else {
        (Pace::OnTrack, "on track".to_string())
    };

    // Ratio-based icon and label (only meaningful once any time has elapsed).
    let (ratio_pace, ratio_label) = if elapsed_pct > 0 {
        let pacing_x100 = (usage_pct * 100) / elapsed_pct;
        let tol = tolerance as i32;
        if pacing_x100 > 100 + tol {
            let dev = (pacing_x100 - 100).min(999);
            (Pace::Ahead, format!("{dev}% ahead"))
        } else if pacing_x100 < 100 - tol {
            let dev = (100 - pacing_x100).min(999);
            (Pace::Under, format!("{dev}% under"))
        } else {
            (Pace::OnTrack, "on track".to_string())
        }
    } else {
        (Pace::OnTrack, "on track".to_string())
    };

    Pacing {
        elapsed_pct,
        ratio_pace,
        point_pace,
        delta,
        ratio_label,
        point_label,
    }
}

/// Color band keyed on signed point delta. Mirrors claudebar's
/// `pace_color_for` (claudebar:212-219). Returns one of the four severity
/// tiers; the caller maps to a theme color.
///
/// `delta <= -10` → low (green); `-10..=0` → mid (yellow);
/// `1..=9` → high (orange); `>= 10` → critical (red).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaceSeverity {
    Low,
    Mid,
    High,
    Critical,
}

pub fn pace_severity(delta: i32) -> PaceSeverity {
    if delta >= 10 {
        PaceSeverity::Critical
    } else if delta > 0 {
        PaceSeverity::High
    } else if delta >= -10 {
        PaceSeverity::Mid
    } else {
        PaceSeverity::Low
    }
}

/// Fill severity for the **pace-marker** feature (the `.continue`
/// "marcador de meta" spec). Colors a usage bar by how far consumption runs
/// *ahead of the meta* — the fraction of the window that has already elapsed —
/// using the user-tunable `--pace-tolerance` band:
///
/// - `delta <= 0` → Low (green): at or under the meta; breathing room.
/// - `0 < delta <= tolerance` → High (amber): slightly ahead; attention.
/// - `delta > tolerance` → Critical (red): over pace; will overrun the reset.
///
/// Deliberately distinct from [`pace_severity`], whose fixed ±10 bands drive
/// the legacy `{*_pace}` *color* placeholders. The marker fill tracks the
/// user's tolerance so the amber band widens/narrows with their setting.
pub fn pace_fill_severity(delta: i32, tolerance: u32) -> PaceSeverity {
    let tol = tolerance as i32;
    if delta <= 0 {
        PaceSeverity::Low
    } else if delta <= tol {
        PaceSeverity::High
    } else {
        PaceSeverity::Critical
    }
}

/// Below this elapsed fraction (percent), a linear reset projection divides by
/// a tiny number and swings wildly, so callers should suppress or soften it.
/// Mirrors the spec's "janela recém-iniciada" caveat (~15%).
pub const MIN_PROJECTION_ELAPSED: i32 = 15;

/// Linear extrapolation of end-of-window usage at the current pace:
/// `usage_pct / elapsed_frac`, i.e. `usage_pct * 100 / elapsed_pct`.
///
/// Returns `None` when too little of the window has elapsed for a stable
/// estimate (`elapsed_pct < MIN_PROJECTION_ELAPSED`), which also guards the
/// divide-by-zero at the window's start. A value `> 100` means "at this pace
/// the quota runs out before the window resets".
pub fn projection_pct(usage_pct: i32, elapsed_pct: i32) -> Option<i32> {
    if elapsed_pct < MIN_PROJECTION_ELAPSED {
        return None;
    }
    Some((usage_pct * 100) / elapsed_pct)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 23, h, m, 0).unwrap()
    }

    const FIVE_H: chrono::Duration = chrono::Duration::hours(5);

    #[test]
    fn missing_reset_returns_neutral() {
        let p = calc(50, None, at(12, 0), FIVE_H, DEFAULT_TOLERANCE);
        assert_eq!(p, Pacing::neutral());
    }

    #[test]
    fn zero_window_returns_neutral() {
        let p = calc(50, Some(at(12, 0)), at(12, 0), chrono::Duration::zero(), 5);
        assert_eq!(p, Pacing::neutral());
    }

    #[test]
    fn elapsed_clamps_to_zero_when_future_reset_beyond_window() {
        // Reset is 6h away but window is 5h → "remaining > total" → negative
        // elapsed → clamped to 0.
        let now = at(12, 0);
        let reset = now + chrono::Duration::hours(6);
        let p = calc(10, Some(reset), now, FIVE_H, 5);
        assert_eq!(p.elapsed_pct, 0);
    }

    #[test]
    fn elapsed_clamps_to_hundred_when_past_reset() {
        let now = at(12, 0);
        let reset = now - chrono::Duration::hours(1);
        let p = calc(50, Some(reset), now, FIVE_H, 5);
        assert_eq!(p.elapsed_pct, 100);
    }

    #[test]
    fn perfectly_even_pacing_is_on_track() {
        // 50% elapsed, 50% usage → both metrics on track.
        let now = at(12, 0);
        let reset = now + chrono::Duration::minutes(150); // 2.5h remain of 5h
        let p = calc(50, Some(reset), now, FIVE_H, DEFAULT_TOLERANCE);
        assert_eq!(p.elapsed_pct, 50);
        assert_eq!(p.delta, 0);
        assert_eq!(p.ratio_pace, Pace::OnTrack);
        assert_eq!(p.point_pace, Pace::OnTrack);
        assert_eq!(p.ratio_label, "on track");
        assert_eq!(p.point_label, "on track");
    }

    #[test]
    fn ahead_of_pace_above_tolerance() {
        // 50% elapsed, 70% usage → delta 20, ratio 140% → "40% ahead".
        let now = at(12, 0);
        let reset = now + chrono::Duration::minutes(150);
        let p = calc(70, Some(reset), now, FIVE_H, 5);
        assert_eq!(p.delta, 20);
        assert_eq!(p.point_pace, Pace::Ahead);
        assert_eq!(p.point_label, "20pts ahead");
        assert_eq!(p.ratio_pace, Pace::Ahead);
        assert_eq!(p.ratio_label, "40% ahead");
    }

    #[test]
    fn under_pace_below_tolerance() {
        // 50% elapsed, 30% usage → delta -20, ratio 60% → "40% under".
        let now = at(12, 0);
        let reset = now + chrono::Duration::minutes(150);
        let p = calc(30, Some(reset), now, FIVE_H, 5);
        assert_eq!(p.delta, -20);
        assert_eq!(p.point_pace, Pace::Under);
        assert_eq!(p.point_label, "20pts under");
        assert_eq!(p.ratio_pace, Pace::Under);
        assert_eq!(p.ratio_label, "40% under");
    }

    #[test]
    fn within_tolerance_band_is_on_track_ratio_but_point_diverges() {
        // 50% elapsed, 52% usage → ratio 104% (within ±5) → on track,
        // BUT point delta is 2 → point_pace = Ahead, point_label "2pts ahead".
        let now = at(12, 0);
        let reset = now + chrono::Duration::minutes(150);
        let p = calc(52, Some(reset), now, FIVE_H, DEFAULT_TOLERANCE);
        assert_eq!(p.ratio_pace, Pace::OnTrack);
        assert_eq!(p.ratio_label, "on track");
        assert_eq!(p.point_pace, Pace::Ahead);
        assert_eq!(p.point_label, "2pts ahead");
    }

    #[test]
    fn ratio_clamps_at_999() {
        // 1% elapsed, 60% usage → pacing_x100 = 6000, dev = 5900 → clamped to 999.
        let now = at(12, 0);
        let reset = now + chrono::Duration::minutes(297); // ~99% remaining → 1% elapsed
        let p = calc(60, Some(reset), now, FIVE_H, 5);
        assert_eq!(p.elapsed_pct, 1);
        assert_eq!(p.ratio_label, "999% ahead");
    }

    #[test]
    fn elapsed_zero_skips_ratio() {
        // 0% elapsed → ratio code is skipped; ratio defaults to on track.
        let now = at(12, 0);
        let reset = now + FIVE_H; // full window remains
        let p = calc(20, Some(reset), now, FIVE_H, 5);
        assert_eq!(p.elapsed_pct, 0);
        assert_eq!(p.ratio_pace, Pace::OnTrack);
        // But point math still runs: delta = 20.
        assert_eq!(p.delta, 20);
        assert_eq!(p.point_pace, Pace::Ahead);
    }

    #[test]
    fn severity_boundaries_match_claudebar() {
        // claudebar: <= -10 green, -10..=0 yellow, 1..9 orange, >= 10 red
        assert_eq!(pace_severity(-100), PaceSeverity::Low);
        assert_eq!(pace_severity(-10), PaceSeverity::Mid); // -10 is in -10..=0 band
        assert_eq!(pace_severity(-1), PaceSeverity::Mid);
        assert_eq!(pace_severity(0), PaceSeverity::Mid);
        assert_eq!(pace_severity(1), PaceSeverity::High);
        assert_eq!(pace_severity(9), PaceSeverity::High);
        assert_eq!(pace_severity(10), PaceSeverity::Critical);
        assert_eq!(pace_severity(100), PaceSeverity::Critical);
    }

    #[test]
    fn pace_fill_severity_maps_delta_to_three_tiers() {
        // delta <= 0 → green (at/under the meta).
        assert_eq!(pace_fill_severity(-50, 5), PaceSeverity::Low);
        assert_eq!(pace_fill_severity(0, 5), PaceSeverity::Low);
        // 0 < delta <= tolerance → amber (attention).
        assert_eq!(pace_fill_severity(1, 5), PaceSeverity::High);
        assert_eq!(pace_fill_severity(5, 5), PaceSeverity::High);
        // delta > tolerance → red (over pace).
        assert_eq!(pace_fill_severity(6, 5), PaceSeverity::Critical);
        assert_eq!(pace_fill_severity(80, 5), PaceSeverity::Critical);
    }

    #[test]
    fn pace_fill_severity_amber_band_tracks_tolerance() {
        // A wider tolerance keeps a larger over-pace delta in the amber band.
        assert_eq!(pace_fill_severity(10, 5), PaceSeverity::Critical);
        assert_eq!(pace_fill_severity(10, 15), PaceSeverity::High);
        // Zero tolerance: any positive delta is immediately red.
        assert_eq!(pace_fill_severity(1, 0), PaceSeverity::Critical);
        assert_eq!(pace_fill_severity(0, 0), PaceSeverity::Low);
    }

    #[test]
    fn projection_suppressed_until_enough_time_elapsed() {
        // Below the stability floor → None (avoids divide-by-tiny + zero).
        assert_eq!(projection_pct(10, 0), None);
        assert_eq!(projection_pct(10, MIN_PROJECTION_ELAPSED - 1), None);
        // At/above the floor → linear extrapolation.
        assert_eq!(projection_pct(10, 50), Some(20)); // 10% used at 50% elapsed → ~20%
        assert_eq!(projection_pct(35, 16), Some(218)); // over pace → projects to overrun
    }

    #[test]
    fn neutral_constructor_matches_default_state() {
        let n = Pacing::neutral();
        assert_eq!(n.elapsed_pct, 0);
        assert_eq!(n.delta, 0);
        assert_eq!(n.ratio_pace, Pace::OnTrack);
        assert_eq!(n.point_pace, Pace::OnTrack);
        assert_eq!(n.ratio_label, "on track");
        assert_eq!(n.point_label, "on track");
    }
}
