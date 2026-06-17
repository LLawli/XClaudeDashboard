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

use ratatui_bubbletea_theme::BubbleTheme;

/// The Charm-inspired theme used for every chrome element in the dashboard.
#[must_use]
pub fn bubble_theme() -> BubbleTheme {
    BubbleTheme::default()
}

#[cfg(test)]
mod tests {
    use super::bubble_theme;
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
}
