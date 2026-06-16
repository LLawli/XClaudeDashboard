use std::collections::BTreeMap;

use ratatui::style::Color;

/// Categorical series palette (per model / per device), curated muted
/// true-color hues (Catppuccin Mocha) that harmonize with the Charm chrome and
/// stay clear of the semantic green/amber/red/accent-pink slots. Assigned by
/// order of appearance via [`ColorMap`].
const PALETTE: [Color; 10] = [
    Color::Rgb(137, 220, 235), // sky
    Color::Rgb(203, 166, 247), // mauve
    Color::Rgb(250, 179, 135), // peach
    Color::Rgb(166, 227, 161), // green
    Color::Rgb(137, 180, 250), // blue
    Color::Rgb(235, 160, 172), // maroon
    Color::Rgb(148, 226, 213), // teal
    Color::Rgb(180, 190, 254), // lavender
    Color::Rgb(249, 226, 175), // yellow
    Color::Rgb(242, 205, 205), // flamingo
];

/// 11th+ series fall back to a muted overlay gray.
const FALLBACK: Color = Color::Rgb(108, 112, 134);

/// Order-of-appearance assignment of palette colors to model names.
/// Re-created on each new 5h window (`reset()`).
#[derive(Debug, Default)]
pub struct ColorMap {
    map: BTreeMap<String, Color>,
}

impl ColorMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the color for `model`, allocating one from the palette in order
    /// of first appearance. Idempotent.
    pub fn assign(&mut self, model: &str) -> Color {
        if let Some(&c) = self.map.get(model) {
            return c;
        }
        let next = PALETTE.get(self.map.len()).copied().unwrap_or(FALLBACK);
        self.map.insert(model.to_string(), next);
        next
    }

    pub fn get(&self, model: &str) -> Option<Color> {
        self.map.get(model).copied()
    }

    pub fn reset(&mut self) {
        self.map.clear();
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assign_idempotent() {
        let mut cm = ColorMap::new();
        let c1 = cm.assign("opus");
        let c2 = cm.assign("opus");
        assert_eq!(c1, c2);
        assert_eq!(cm.len(), 1);
    }

    #[test]
    fn first_three_models_get_first_three_colors() {
        let mut cm = ColorMap::new();
        assert_eq!(cm.assign("opus"), PALETTE[0]);
        assert_eq!(cm.assign("sonnet"), PALETTE[1]);
        assert_eq!(cm.assign("haiku"), PALETTE[2]);
    }

    #[test]
    fn eleventh_model_falls_back() {
        let mut cm = ColorMap::new();
        for i in 0..10 {
            cm.assign(&format!("m{i}"));
        }
        assert_eq!(cm.assign("eleven"), FALLBACK);
    }

    #[test]
    fn reset_returns_to_first_slot() {
        let mut cm = ColorMap::new();
        cm.assign("a");
        cm.assign("b");
        cm.reset();
        assert!(cm.is_empty());
        assert_eq!(cm.assign("a"), PALETTE[0]);
    }

    #[test]
    fn get_returns_none_for_unmapped() {
        let cm = ColorMap::new();
        assert!(cm.get("nope").is_none());
    }
}
