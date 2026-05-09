use std::collections::BTreeMap;

use ratatui::style::Color;

const PALETTE: [Color; 10] = [
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::Red,
    Color::LightCyan,
    Color::LightMagenta,
    Color::LightYellow,
    Color::LightGreen,
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
        assert_eq!(cm.assign("opus"), Color::Cyan);
        assert_eq!(cm.assign("sonnet"), Color::Magenta);
        assert_eq!(cm.assign("haiku"), Color::Yellow);
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
        assert_eq!(cm.assign("a"), Color::Cyan);
    }

    #[test]
    fn get_returns_none_for_unmapped() {
        let cm = ColorMap::new();
        assert!(cm.get("nope").is_none());
    }
}
