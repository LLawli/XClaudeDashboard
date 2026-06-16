//! Charm/Bubble Tea theme adapter + shared semantic helpers.
//!
//! Centralizes the single [`BubbleTheme`] used for all *chrome* (borders, tabs,
//! footer, header, gauge, spinner) plus the severity color ramp shared by the
//! hero gauge fill and the ETA text so they never drift. Per-series colors (one
//! per model or device) deliberately stay in [`crate::colors`].
//!
//! The theme is cheap to build (`Copy`, all-`const` palette), so callers just
//! call [`bubble_theme`] inside each render function instead of threading a
//! reference everywhere.

use ratatui::style::{Color, Style};
use ratatui_bubbletea_theme::BubbleTheme;

/// The Charm-inspired theme used for every chrome element in the dashboard.
#[must_use]
pub fn bubble_theme() -> BubbleTheme {
    BubbleTheme::default()
}

/// Severity color for burn-rate headroom, drawn from the Charm palette:
/// muted (idle / past reset), success (won't overrun the reset), warning
/// (tight), error (burning fast). Shared by the hero gauge fill and the ETA
/// value so the two can never disagree.
#[must_use]
pub fn severity_color(eta_seconds: Option<u64>, until_reset: i64) -> Color {
    let p = bubble_theme().palette;
    match eta_seconds {
        None => p.muted,
        Some(_) if until_reset <= 0 => p.muted,
        Some(s) if (s as i64) >= until_reset => p.success,
        Some(s) if (s as i64) >= until_reset / 2 => p.warning,
        Some(_) => p.error,
    }
}

/// A copy of the theme whose `accent` style (the color the `Progress` widget
/// uses for its filled segment) is swapped to `color`. Lets the hero gauge
/// render its fill in the severity color while keeping the muted track.
#[must_use]
pub fn accent_theme(color: Color) -> BubbleTheme {
    let mut theme = bubble_theme();
    theme.accent = Style::new().fg(color);
    theme
}

#[cfg(test)]
mod tests {
    use super::{accent_theme, bubble_theme, severity_color};
    use ratatui_bubbletea_theme::Palette;

    #[test]
    fn theme_uses_charm_palette() {
        assert_eq!(bubble_theme().palette, Palette::CHARM);
    }

    #[test]
    fn severity_idle_and_after_reset_are_muted() {
        assert_eq!(severity_color(None, 1_000), Palette::CHARM.muted);
        assert_eq!(severity_color(Some(60), -10), Palette::CHARM.muted);
    }

    #[test]
    fn severity_wont_overflow_is_success() {
        assert_eq!(severity_color(Some(2_000), 1_000), Palette::CHARM.success);
        assert_eq!(severity_color(Some(1_000), 1_000), Palette::CHARM.success);
    }

    #[test]
    fn severity_tight_is_warning() {
        // 600 < 1000 but >= 500 → warning band
        assert_eq!(severity_color(Some(600), 1_000), Palette::CHARM.warning);
    }

    #[test]
    fn severity_burning_is_error() {
        // 100 < 500 → error band
        assert_eq!(severity_color(Some(100), 1_000), Palette::CHARM.error);
    }

    #[test]
    fn accent_theme_swaps_fill_color_only() {
        let t = accent_theme(Palette::CHARM.success);
        assert_eq!(t.accent.fg, Some(Palette::CHARM.success));
        // Track color (muted) is untouched.
        assert_eq!(t.muted.fg, Some(Palette::CHARM.muted));
    }
}
