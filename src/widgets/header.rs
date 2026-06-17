use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::format::{compact, duration_hms, iso_local_dated_hms, iso_local_hms};

pub struct HeaderState {
    pub title: &'static str,
    /// `true` shows weekday + day-of-month before each timestamp (used by the
    /// 7d view, where `started` and `resets` share an `HH:MM:SS`).
    pub show_date: bool,
    pub started: i64,
    pub resets_at: i64,
    pub now: i64,
    pub used_pct: f64,
    pub output_used: u64,
    pub output_limit: u64,
    pub eta_seconds: Option<u64>,
    pub eta_color: Color,
    pub rate_per_min: f64,
    pub rate_window_min: u32,
}

impl Widget for &HeaderState {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = crate::style::bubble_theme();
        let stamp = if self.show_date {
            iso_local_dated_hms
        } else {
            iso_local_hms
        };
        let session = Line::from(vec![Span::styled(
            format!(
                " started {} · resets {} · {} left ",
                stamp(self.started),
                stamp(self.resets_at),
                duration_hms(self.resets_at - self.now),
            ),
            theme.text.add_modifier(Modifier::BOLD),
        )]);

        let remaining = self.output_limit.saturating_sub(self.output_used);

        let output = Line::from(format!(
            "  output     {} / {}    {:.1}%",
            compact(self.output_used),
            compact(self.output_limit),
            self.used_pct,
        ));
        let remaining_line = Line::from(format!("  remaining  {}", compact(remaining)));

        let until_reset = self.resets_at - self.now;
        let eta_text = match self.eta_seconds {
            Some(s) if until_reset > 0 && (s as i64) >= until_reset => format!(
                "> reset (rate {}/min · last {}min)",
                compact(self.rate_per_min as u64),
                self.rate_window_min,
            ),
            Some(s) => format!(
                "{}  (rate {}/min · last {}min)",
                duration_hms(s as i64),
                compact(self.rate_per_min as u64),
                self.rate_window_min,
            ),
            None => "—  (idle)".into(),
        };
        let eta = Line::from(vec![
            Span::raw("  ETA 100%   "),
            Span::styled(eta_text, Style::default().fg(self.eta_color)),
        ]);

        let para = Paragraph::new(vec![session, output, remaining_line, eta])
            .block(theme.titled_block(self.title));
        para.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer_text(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn render(state: &HeaderState, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        Widget::render(state, area, &mut buf);
        buf
    }

    fn sample(eta: Option<u64>) -> HeaderState {
        HeaderState {
            title: " session 5h ",
            show_date: false,
            started: 1_000_000,
            resets_at: 1_000_000 + 18_000,
            now: 1_000_000 + 100,
            used_pct: 5.5,
            output_used: 1_234,
            output_limit: 22_000,
            eta_seconds: eta,
            eta_color: Color::Green,
            rate_per_min: 200.0,
            rate_window_min: 15,
        }
    }

    #[test]
    fn renders_session_metadata_lines() {
        let s = sample(Some(7_200));
        let buf = render(&s, 80, 6);
        let text = buffer_text(&buf);
        assert!(text.contains("session 5h"), "block title missing: {text}");
        assert!(text.contains("started"), "started missing: {text}");
        assert!(text.contains("resets"), "resets missing: {text}");
        assert!(text.contains("left"), "left missing: {text}");
    }

    #[test]
    fn renders_custom_title() {
        let mut s = sample(Some(7_200));
        s.title = " session 7d ";
        let buf = render(&s, 80, 6);
        let text = buffer_text(&buf);
        assert!(
            text.contains("session 7d"),
            "expected 7d title; got: {text}"
        );
        assert!(!text.contains("session 5h"), "should not show 5h: {text}");
    }

    #[test]
    fn renders_output_remaining_eta_lines() {
        let s = sample(Some(7_200));
        let buf = render(&s, 80, 6);
        let text = buffer_text(&buf);
        assert!(text.contains("output"));
        assert!(text.contains("remaining"));
        assert!(text.contains("ETA 100%"));
        assert!(text.contains("rate"));
    }

    #[test]
    fn renders_idle_dash_when_eta_is_none() {
        let s = sample(None);
        let buf = render(&s, 80, 6);
        let text = buffer_text(&buf);
        assert!(text.contains("—"), "expected idle dash; got: {text}");
        assert!(text.contains("idle"));
    }

    #[test]
    fn renders_overflow_marker_when_eta_exceeds_window() {
        let mut s = sample(Some(60_000)); // ETA way past reset
        s.now = s.resets_at - 1_000; // 1000 secs left
        let buf = render(&s, 80, 6);
        let text = buffer_text(&buf);
        assert!(
            text.contains("> reset"),
            "expected overflow marker; got: {text}"
        );
    }

    #[test]
    fn does_not_panic_with_negative_until_reset() {
        let mut s = sample(Some(60));
        s.now = s.resets_at + 100; // window already past
        let _ = render(&s, 80, 6); // must not panic; duration_hms clamps
    }

    #[test]
    fn does_not_panic_with_zero_output_limit() {
        let mut s = sample(Some(0));
        s.output_limit = 0;
        let _ = render(&s, 80, 6); // saturating_sub keeps it sane
    }
}
