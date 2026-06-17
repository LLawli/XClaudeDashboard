use std::collections::BTreeMap;

use ratatui::style::Color;

// A cohesive categorical palette (one locked-ish lightness/chroma, rotating
// hue) for per-model / per-device series. It deliberately LEADS with hues that
// are not in the Charm chrome's semantic slots (teal, not the chrome blue;
// orange; violet) so "data identity" and "chrome status" read as two different
// visual languages.
const PALETTE: [Color; 10] = [
    Color::Rgb(45, 212, 191),  // teal
    Color::Rgb(251, 146, 60),  // orange
    Color::Rgb(167, 139, 250), // violet
    Color::Rgb(163, 230, 53),  // lime
    Color::Rgb(232, 121, 249), // fuchsia
    Color::Rgb(56, 189, 248),  // sky
    Color::Rgb(250, 204, 21),  // yellow
    Color::Rgb(129, 140, 248), // indigo
    Color::Rgb(52, 211, 153),  // emerald
    Color::Rgb(244, 114, 182), // pink
];

const FALLBACK: Color = Color::DarkGray;

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
        assert_eq!(cm.assign("opus"), Color::Rgb(45, 212, 191));
        assert_eq!(cm.assign("sonnet"), Color::Rgb(251, 146, 60));
        assert_eq!(cm.assign("haiku"), Color::Rgb(167, 139, 250));
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
        assert_eq!(cm.assign("a"), Color::Rgb(45, 212, 191));
    }

    #[test]
    fn get_returns_none_for_unmapped() {
        let cm = ColorMap::new();
        assert!(cm.get("nope").is_none());
    }
}
