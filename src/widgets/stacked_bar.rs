use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

const FILL: &str = "█";

/// Horizontal stacked bar. Each `(color, fraction)` slice is drawn in
/// proportion to its fraction over the sum. Fractions need not sum to 1 —
/// they're normalized by the total. Rounding error from per-cell allocation
/// is absorbed by the last slice so the row is always fully painted.
pub struct StackedBar<'a> {
    pub slices: &'a [(Color, f64)],
}

impl Widget for StackedBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 || self.slices.is_empty() {
            return;
        }
        let total_cells = area.width as usize;
        let total_fraction: f64 = self.slices.iter().map(|(_, f)| f).sum();
        if total_fraction <= 0.0 {
            return;
        }

        let mut alloc: Vec<usize> = self
            .slices
            .iter()
            .map(|(_, f)| ((f / total_fraction) * total_cells as f64).round() as usize)
            .collect();

        let assigned: usize = alloc.iter().sum();
        let n = alloc.len();
        if assigned < total_cells {
            alloc[n - 1] += total_cells - assigned;
        } else if assigned > total_cells {
            let excess = assigned - total_cells;
            alloc[n - 1] = alloc[n - 1].saturating_sub(excess);
        }

        let y = area.y;
        let mut x = area.x;
        for ((color, _), cells) in self.slices.iter().zip(alloc.iter()) {
            if *cells == 0 {
                continue;
            }
            let s: String = FILL.repeat(*cells);
            buf.set_string(x, y, &s, Style::default().fg(*color));
            x += *cells as u16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(slices: &[(Color, f64)], width: u16) -> Buffer {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        StackedBar { slices }.render(area, &mut buf);
        buf
    }

    fn fg_at(buf: &Buffer, x: u16) -> Color {
        buf[(x, 0)].fg
    }

    #[test]
    fn empty_slices_do_nothing() {
        let buf = render(&[], 10);
        for x in 0..10 {
            assert_eq!(fg_at(&buf, x), Color::Reset);
        }
    }

    #[test]
    fn fifty_fifty_split_paints_half_and_half() {
        let buf = render(&[(Color::Red, 0.5), (Color::Green, 0.5)], 10);
        for x in 0..5 {
            assert_eq!(fg_at(&buf, x), Color::Red, "cell {x}");
        }
        for x in 5..10 {
            assert_eq!(fg_at(&buf, x), Color::Green, "cell {x}");
        }
    }

    #[test]
    fn three_slices_normalize_correctly() {
        // 1, 2, 1 → ratios .25, .5, .25 over 8 cells = 2, 4, 2
        let slices = [(Color::Red, 1.0), (Color::Green, 2.0), (Color::Blue, 1.0)];
        let buf = render(&slices, 8);
        assert_eq!(fg_at(&buf, 0), Color::Red);
        assert_eq!(fg_at(&buf, 1), Color::Red);
        assert_eq!(fg_at(&buf, 2), Color::Green);
        assert_eq!(fg_at(&buf, 5), Color::Green);
        assert_eq!(fg_at(&buf, 6), Color::Blue);
        assert_eq!(fg_at(&buf, 7), Color::Blue);
    }

    #[test]
    fn last_slice_absorbs_rounding_to_fill_full_width() {
        // 3 slices of 1/3 each over 10 cells: round(10/3)=3, 3, 3 → assigned 9, last gets +1
        let slices = [(Color::Red, 1.0), (Color::Green, 1.0), (Color::Blue, 1.0)];
        let buf = render(&slices, 10);
        let mut counts = [0usize; 3];
        for x in 0..10 {
            match fg_at(&buf, x) {
                Color::Red => counts[0] += 1,
                Color::Green => counts[1] += 1,
                Color::Blue => counts[2] += 1,
                _ => {}
            }
        }
        let total: usize = counts.iter().sum();
        assert_eq!(total, 10, "all 10 cells must be painted, got {counts:?}");
    }

    #[test]
    fn zero_total_fraction_paints_nothing() {
        let buf = render(&[(Color::Red, 0.0), (Color::Green, 0.0)], 10);
        for x in 0..10 {
            assert_eq!(fg_at(&buf, x), Color::Reset);
        }
    }

    #[test]
    fn handles_zero_width_area() {
        let area = Rect::new(0, 0, 0, 1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        StackedBar {
            slices: &[(Color::Red, 1.0)],
        }
        .render(area, &mut buf);
        assert_eq!(fg_at(&buf, 0), Color::Reset);
    }

    #[test]
    fn renders_full_block_glyph() {
        let buf = render(&[(Color::Red, 1.0)], 3);
        for x in 0..3 {
            assert_eq!(buf[(x, 0)].symbol(), FILL);
        }
    }
}
