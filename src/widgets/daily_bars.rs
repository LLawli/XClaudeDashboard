use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::bar;
use ratatui::widgets::Widget;

/// One column of the per-day chart.
pub struct DailyBar {
    pub label: String,
    pub value: f64,
    pub color: Color,
    pub label_style: Style,
}

/// Vertical bars with the value printed just above each bar and the label on
/// the bottom row. Unlike ratatui's `BarChart`, the value never overlays the
/// bar, so a near-empty bar still reads as near-empty.
pub struct DailyBars<'a> {
    pub bars: &'a [DailyBar],
}

const GAP: u16 = 1;
const EIGHTHS: [&str; 9] = [
    " ",
    bar::ONE_EIGHTH,
    bar::ONE_QUARTER,
    bar::THREE_EIGHTHS,
    bar::HALF,
    bar::FIVE_EIGHTHS,
    bar::THREE_QUARTERS,
    bar::SEVEN_EIGHTHS,
    bar::FULL,
];

impl Widget for DailyBars<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let n = self.bars.len() as u16;
        // Rows: value text above the tallest bar, the bars, the labels.
        if n == 0 || area.height < 3 || area.width < n * (1 + GAP) - GAP {
            return;
        }
        let col_w = ((area.width + GAP) / n).saturating_sub(GAP).max(1);
        let used_w = n * col_w + (n - 1) * GAP;
        let left = area.x + area.width.saturating_sub(used_w) / 2;
        let bars_h = area.height - 2;
        let label_y = area.bottom() - 1;
        let max = self
            .bars
            .iter()
            .map(|b| b.value)
            .filter(|v| v.is_finite())
            .fold(0.0_f64, f64::max);

        for (i, b) in self.bars.iter().enumerate() {
            let x = left + i as u16 * (col_w + GAP);
            let ratio = if max > 0.0 && b.value.is_finite() {
                (b.value / max).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut eighths = (ratio * f64::from(bars_h) * 8.0).round() as u16;
            if b.value > 0.0 && eighths == 0 {
                eighths = 1; // any spend stays visible
            }
            let style = Style::default().fg(b.color);
            for row in 0..bars_h {
                let fill = eighths.saturating_sub(row * 8).min(8) as usize;
                if fill == 0 {
                    break;
                }
                let y = label_y - 1 - row;
                for dx in 0..col_w {
                    buf[(x + dx, y)].set_symbol(EIGHTHS[fill]).set_style(style);
                }
            }
            // Value sits on the first empty row above the bar top.
            let top_row = eighths.div_ceil(8);
            let value_y = label_y - 1 - top_row.min(bars_h);
            put_centered(
                buf,
                x,
                value_y,
                col_w,
                &crate::format::pct_short(b.value),
                style,
            );
            put_centered(buf, x, label_y, col_w, &b.label, b.label_style);
        }
    }
}

fn put_centered(buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    let len = text.chars().count() as u16;
    let pad = width.saturating_sub(len) / 2;
    buf.set_stringn(x + pad, y, text, width as usize, style);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars() -> Vec<DailyBar> {
        [("Sun", 28.1), ("Mon", 33.7), ("Tue", 0.6)]
            .into_iter()
            .map(|(l, v)| DailyBar {
                label: l.into(),
                value: v,
                color: Color::White,
                label_style: Style::default(),
            })
            .collect()
    }

    fn text(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn labels_on_bottom_row_and_values_above_bars() {
        let area = Rect::new(0, 0, 20, 8);
        let mut buf = Buffer::empty(area);
        let b = bars();
        DailyBars { bars: &b }.render(area, &mut buf);
        let rows = text(&buf);
        let last = rows.last().unwrap();
        assert!(last.contains("Sun") && last.contains("Mon") && last.contains("Tue"));
        // Tallest bar fills every bar row, so its value lands on the top row.
        assert!(rows[0].contains("34%"), "{rows:?}");
        // The tiny bar keeps a sliver and its value sits right above it.
        assert!(rows[rows.len() - 3].contains("0.6%"), "{rows:?}");
    }

    #[test]
    fn degenerate_sizes_do_not_panic() {
        let b = bars();
        for (w, h) in [(0, 0), (1, 1), (3, 3), (2, 10), (200, 2), (5, 40)] {
            let area = Rect::new(0, 0, w, h);
            let mut buf = Buffer::empty(area);
            DailyBars { bars: &b }.render(area, &mut buf);
        }
        let area = Rect::new(0, 0, 20, 8);
        let mut buf = Buffer::empty(area);
        DailyBars { bars: &[] }.render(area, &mut buf);
        let nan = [DailyBar {
            label: "x".into(),
            value: f64::NAN,
            color: Color::White,
            label_style: Style::default(),
        }];
        DailyBars { bars: &nan }.render(area, &mut buf);
    }
}
