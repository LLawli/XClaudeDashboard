use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Padding, Paragraph, Widget};
use ratatui_bubbletea_components::{Progress, ProgressSymbols};

use crate::format::{compact, duration_hms, iso_local_dated_hms, iso_local_hms};

/// The hero "session" card: a rounded titled panel whose body is a big
/// utilization gauge (`% used` + a severity-colored `Progress` bar + the
/// output/limit), with a single metadata caption line beneath. The reset
/// countdown (or, while syncing, an animated spinner note) rides the top border
/// on the right.
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
    /// Severity color (muted/success/warning/error), shared with the gauge fill.
    pub eta_color: Color,
    pub rate_per_min: f64,
    pub rate_window_min: u32,
    /// While a background sync is in flight, the right-border annotation shows
    /// this note (e.g. `⠋ syncing remote…`) instead of the reset countdown.
    pub spinner_note: Option<String>,
}

impl Widget for &HeaderState {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = crate::style::bubble_theme();
        let stamp = if self.show_date {
            iso_local_dated_hms
        } else {
            iso_local_hms
        };
        let until_reset = self.resets_at - self.now;
        let remaining = self.output_limit.saturating_sub(self.output_used);

        // Top-border right annotation: spinner note while syncing, else the
        // reset countdown.
        let annotation = match &self.spinner_note {
            Some(note) => Span::styled(note.clone(), theme.accent),
            None => Span::styled(
                format!(
                    " resets {} · {} left ",
                    stamp(self.resets_at),
                    duration_hms(until_reset),
                ),
                theme.muted,
            ),
        };
        let block = theme
            .titled_block(self.title)
            .title_style(crate::style::heading())
            .title_top(Line::from(annotation).right_aligned())
            .padding(Padding::horizontal(2));
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.width == 0 || inner.height < 4 {
            return;
        }

        // 4 inner rows: blank · gauge · blank · meta.
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

        // --- Gauge row: "15% used" | ▰▰▱▱ | "1.1M / 7.1M" ---
        let gcols = Layout::horizontal([
            Constraint::Length(13),
            Constraint::Min(8),
            Constraint::Length(16),
        ])
        .split(rows[1]);

        let pct_label = Line::from(vec![
            Span::styled(
                format!("{:.0}%", self.used_pct),
                Style::new().fg(self.eta_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" used", theme.muted),
        ]);
        Paragraph::new(pct_label).render(gcols[0], buf);

        let pct = self.used_pct.round().clamp(0.0, 100.0) as u16;
        let gauge = Progress::from_percent(pct)
            .show_percentage(false)
            .symbols(ProgressSymbols {
                filled: "▰",
                empty: "▱",
            })
            .theme(crate::style::accent_theme(self.eta_color));
        Widget::render(&gauge, gcols[1], buf);

        let tokens = format!(
            "{} / {}",
            compact(self.output_used),
            compact(self.output_limit),
        );
        Paragraph::new(Line::from(Span::styled(tokens, theme.text)))
            .alignment(Alignment::Right)
            .render(gcols[2], buf);

        // --- Meta row ---
        let eta_status = match self.eta_seconds {
            Some(s) if until_reset > 0 && (s as i64) >= until_reset => "> reset".to_string(),
            Some(s) => duration_hms(s as i64),
            None => "—".to_string(),
        };
        let lead = match self.eta_seconds {
            None => "idle · ".to_string(),
            Some(_) => String::new(),
        };
        let details = format!(
            "{lead}rate {}/min · last {}min · remaining {} · started {}",
            compact(self.rate_per_min as u64),
            self.rate_window_min,
            compact(remaining),
            stamp(self.started),
        );
        let meta = Line::from(vec![
            Span::styled("eta 100% ", theme.muted),
            Span::styled(eta_status, Style::new().fg(self.eta_color)),
            Span::styled("  ·  ", theme.muted),
            Span::styled(details, theme.muted),
        ]);
        Paragraph::new(meta).render(rows[3], buf);
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
            used_pct: 15.0,
            output_used: 1_100_000,
            output_limit: 7_100_000,
            eta_seconds: eta,
            eta_color: Color::Green,
            rate_per_min: 29_000.0,
            rate_window_min: 15,
            spinner_note: None,
        }
    }

    #[test]
    fn renders_session_title_and_reset_annotation() {
        let s = sample(Some(7_200));
        let text = buffer_text(&render(&s, 90, 6));
        assert!(text.contains("session 5h"), "block title missing: {text}");
        assert!(text.contains("resets"), "resets annotation missing: {text}");
        assert!(text.contains("left"), "left missing: {text}");
    }

    #[test]
    fn renders_custom_title() {
        let mut s = sample(Some(7_200));
        s.title = " session 7d ";
        let text = buffer_text(&render(&s, 90, 6));
        assert!(text.contains("session 7d"), "expected 7d title; got: {text}");
        assert!(!text.contains("session 5h"), "should not show 5h: {text}");
    }

    #[test]
    fn renders_gauge_caption_and_meta_line() {
        let s = sample(Some(7_200));
        let text = buffer_text(&render(&s, 90, 6));
        assert!(text.contains("used"), "gauge 'used' label missing: {text}");
        assert!(text.contains('%'), "percent missing: {text}");
        assert!(text.contains('/'), "output/limit slash missing: {text}");
        assert!(text.contains("eta 100%"), "eta label missing: {text}");
        assert!(text.contains("rate"), "rate missing: {text}");
        assert!(text.contains("remaining"), "remaining missing: {text}");
        assert!(text.contains("started"), "started missing: {text}");
    }

    #[test]
    fn renders_idle_dash_when_eta_is_none() {
        let text = buffer_text(&render(&sample(None), 90, 6));
        assert!(text.contains("—"), "expected idle dash; got: {text}");
        assert!(text.contains("idle"), "expected idle word; got: {text}");
    }

    #[test]
    fn renders_overflow_marker_when_eta_exceeds_window() {
        let mut s = sample(Some(60_000)); // ETA way past reset
        s.now = s.resets_at - 1_000; // 1000 secs left
        let text = buffer_text(&render(&s, 90, 6));
        assert!(text.contains("> reset"), "expected overflow marker; got: {text}");
    }

    #[test]
    fn renders_spinner_note_when_syncing() {
        let mut s = sample(Some(7_200));
        s.spinner_note = Some("⠋ syncing remote…".to_string());
        let text = buffer_text(&render(&s, 90, 6));
        assert!(text.contains("syncing"), "expected spinner note; got: {text}");
        // The reset countdown is replaced by the note.
        assert!(!text.contains("left"), "countdown should be hidden while syncing: {text}");
    }

    #[test]
    fn does_not_panic_with_negative_until_reset() {
        let mut s = sample(Some(60));
        s.now = s.resets_at + 100; // window already past
        let _ = render(&s, 90, 6); // must not panic; duration_hms clamps
    }

    #[test]
    fn does_not_panic_with_zero_output_limit() {
        let mut s = sample(Some(0));
        s.output_limit = 0;
        let _ = render(&s, 90, 6); // saturating_sub keeps it sane
    }

    #[test]
    fn does_not_panic_on_tiny_area() {
        let _ = render(&sample(Some(7_200)), 10, 3);
    }
}
