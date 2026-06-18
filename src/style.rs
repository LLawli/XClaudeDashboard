//! Charm/Bubble Tea theme adapter.
//!
//! Centralizes the single [`BubbleTheme`] used for all *chrome* (borders, tabs,
//! footer, header, severity colors, spinner). Per-series colors (one per model
//! or device) deliberately stay in [`crate::colors`]: those are a categorical
//! palette assigned by order of appearance and asserted by tests, so they must
//! not collide with the theme's semantic slots.
//!
//! The theme is cheap to build (`Copy`, all-`const` palette), so callers just
//! call [`bubble_theme`] inside each render function instead of threading a
//! reference everywhere.

use ratatui::style::Color;
use ratatui_bubbletea_theme::BubbleTheme;

/// The Charm-inspired theme used for every chrome element in the dashboard.
#[must_use]
pub fn bubble_theme() -> BubbleTheme {
    BubbleTheme::default()
}

/// Severity color for the usage gauge by fill level: green under 70%, amber
/// 70–90%, red at/over 90%.
#[must_use]
pub fn usage_severity(used_pct: f64) -> Color {
    let p = bubble_theme().palette;
    if used_pct >= 90.0 {
        p.error
    } else if used_pct >= 70.0 {
        p.warning
    } else {
        p.success
    }
}

/// Severity color for the ETA-to-100%: muted when idle or after the reset,
/// success when the budget lasts past the reset, warning/error as it tightens.
#[must_use]
pub fn eta_color(eta_seconds: Option<u64>, until_reset: i64) -> Color {
    let p = bubble_theme().palette;
    match eta_seconds {
        None => p.muted,
        Some(_) if until_reset <= 0 => p.muted,
        Some(s) if (s as i64) >= until_reset => p.success,
        Some(s) if (s as i64) >= until_reset / 2 => p.warning,
        Some(_) => p.error,
    }
}

/// 3-stop heat ramp for the burn sparkline: muted (idle) → foreground (normal)
/// → amber (spike), by a column's share of the window peak. Color is redundant
/// with bar height, so the chart stays legible on low-color terminals.
#[must_use]
pub fn heat_color(ratio: f64) -> Color {
    let stops = [(122u8, 122, 122), (230, 230, 230), (255, 193, 7)];
    let r = ratio.clamp(0.0, 1.0);
    let (lo, hi, t) = if r < 0.5 {
        (stops[0], stops[1], r * 2.0)
    } else {
        (stops[1], stops[2], (r - 0.5) * 2.0)
    };
    let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
    Color::Rgb(mix(lo.0, hi.0), mix(lo.1, hi.1), mix(lo.2, hi.2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_bubbletea_theme::Palette;

    #[test]
    fn theme_uses_charm_palette() {
        assert_eq!(bubble_theme().palette, Palette::CHARM);
    }

    #[test]
    fn semantic_styles_derive_from_palette() {
        let theme = bubble_theme();
        assert_eq!(theme.success.fg, Some(Palette::CHARM.success));
        assert_eq!(theme.warning.fg, Some(Palette::CHARM.warning));
        assert_eq!(theme.error.fg, Some(Palette::CHARM.error));
        assert_eq!(theme.muted.fg, Some(Palette::CHARM.muted));
    }

    #[test]
    fn usage_severity_bands() {
        let p = bubble_theme().palette;
        assert_eq!(usage_severity(0.0), p.success);
        assert_eq!(usage_severity(69.9), p.success);
        assert_eq!(usage_severity(70.0), p.warning);
        assert_eq!(usage_severity(89.9), p.warning);
        assert_eq!(usage_severity(90.0), p.error);
        assert_eq!(usage_severity(150.0), p.error);
    }

    #[test]
    fn eta_color_bands() {
        let p = bubble_theme().palette;
        assert_eq!(eta_color(None, 1_000), p.muted); // idle
        assert_eq!(eta_color(Some(2_000), 1_000), p.success); // lasts past reset
        assert_eq!(eta_color(Some(1_000), 1_000), p.success);
        assert_eq!(eta_color(Some(600), 1_000), p.warning); // >= half the window
        assert_eq!(eta_color(Some(100), 1_000), p.error); // burning
        assert_eq!(eta_color(Some(60), -10), p.muted); // after reset
    }

    #[test]
    fn heat_color_ramps_muted_through_fg_to_amber() {
        assert_eq!(heat_color(0.0), Color::Rgb(122, 122, 122));
        assert_eq!(heat_color(0.5), Color::Rgb(230, 230, 230));
        assert_eq!(heat_color(1.0), Color::Rgb(255, 193, 7));
        // out-of-range clamps to the endpoints, never panics
        assert_eq!(heat_color(-1.0), Color::Rgb(122, 122, 122));
        assert_eq!(heat_color(2.0), Color::Rgb(255, 193, 7));
    }
}
