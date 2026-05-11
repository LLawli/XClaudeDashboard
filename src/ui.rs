use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, FooterStatus, IDLE_THRESHOLD_PER_MIN, RATE_WINDOW_MIN, Status};
use crate::pricing::cost_for;
use crate::widgets::header::HeaderState;
use crate::widgets::legend::{DeviceRow, LegendRow, build_device_table, build_table};
use crate::widgets::stacked_bar::StackedBar;
use crate::window::WindowKind;

/// Tabs row is always the first row of the frame.
pub const TABS_ROW: u16 = 0;

/// Hit-test for a click on the tabs row. Each tab claims half of the available
/// width; clicks inside the row always resolve to one tab or the other (no
/// gap), but clicks past `width` (or with `width == 0`) return `None`.
pub fn tab_hit(col: u16, width: u16) -> Option<WindowKind> {
    if width == 0 || col >= width {
        return None;
    }
    if col < width / 2 {
        Some(WindowKind::FiveHour)
    } else {
        Some(WindowKind::SevenDay)
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tabs
            Constraint::Length(6), // header block (4 lines + 2 borders)
            Constraint::Length(1), // stacked bar
            Constraint::Length(1), // spacer
            Constraint::Min(5),    // legend
            Constraint::Length(1), // footer
        ])
        .split(frame.area());

    render_tabs(frame, app, chunks[0]);
    render_header(frame, app, chunks[1]);
    render_stacked_bar(frame, app, chunks[2]);
    render_legend(frame, app, chunks[4]);
    render_footer(frame, app, chunks[5]);
}

fn render_tabs(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let active = Style::default()
        .bg(Color::DarkGray)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(Color::DarkGray);
    let (style_5h, style_7d) = match app.view {
        WindowKind::FiveHour => (active, inactive),
        WindowKind::SevenDay => (inactive, active),
    };

    let tab_5h = Paragraph::new("5h (h)")
        .alignment(Alignment::Center)
        .style(style_5h);
    let tab_7d = Paragraph::new("7d (s)")
        .alignment(Alignment::Center)
        .style(style_7d);

    frame.render_widget(tab_5h, halves[0]);
    frame.render_widget(tab_7d, halves[1]);
}

fn render_header(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = view_title(app.view);
    let Some(window) = app.window else {
        let block = Block::bordered().title(title);
        let para = Paragraph::new("waiting for first hook from XClaudeUsage…").block(block);
        frame.render_widget(para, area);
        return;
    };

    let output_used = app.aggregate.total_output;
    let output_limit = output_limit(output_used, window.used_percentage);
    let remaining = output_limit.saturating_sub(output_used);

    let eta_seconds = app.rate.eta_seconds(
        remaining,
        app.now_secs,
        RATE_WINDOW_MIN,
        IDLE_THRESHOLD_PER_MIN,
    );
    let until_reset = window.resets_at - app.now_secs;
    let eta_color = eta_color(eta_seconds, until_reset);

    let state = HeaderState {
        title,
        show_date: matches!(app.view, WindowKind::SevenDay),
        started: window.start_at,
        resets_at: window.resets_at,
        now: app.now_secs,
        used_pct: window.used_percentage,
        output_used,
        output_limit,
        eta_seconds,
        eta_color,
        rate_per_min: app.rate.rate_per_minute(app.now_secs, RATE_WINDOW_MIN),
        rate_window_min: RATE_WINDOW_MIN,
    };
    frame.render_widget(&state, area);
}

fn view_title(view: WindowKind) -> &'static str {
    match view {
        WindowKind::FiveHour => " session 5h ",
        WindowKind::SevenDay => " session 7d ",
    }
}

fn render_stacked_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.verbose {
        let total = app.device_aggregate.grand_total_tokens();
        if total == 0 {
            return;
        }
        let slices: Vec<(Color, f64)> = app
            .device_aggregate
            .per_device
            .keys()
            .map(|device| {
                let color = app.device_colors.get(device).unwrap_or(Color::DarkGray);
                let frac = app.device_aggregate.totals(device).total() as f64 / total as f64;
                (color, frac)
            })
            .collect();
        frame.render_widget(StackedBar { slices: &slices }, area);
        return;
    }
    let total: u64 = app.aggregate.per_model.values().map(|t| t.total()).sum();
    if total == 0 {
        return;
    }
    let slices: Vec<(Color, f64)> = app
        .aggregate
        .per_model
        .iter()
        .map(|(model, totals)| {
            let color = app.colors.get(model).unwrap_or(Color::DarkGray);
            (color, totals.total() as f64 / total as f64)
        })
        .collect();
    frame.render_widget(StackedBar { slices: &slices }, area);
}

fn render_legend(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.verbose {
        render_device_legend(frame, app, area);
    } else {
        render_model_legend(frame, app, area);
    }
}

fn render_model_legend(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let total: u64 = app.aggregate.per_model.values().map(|t| t.total()).sum();
    let rows: Vec<LegendRow> = app
        .aggregate
        .per_model
        .iter()
        .map(|(model, totals)| {
            let color = app.colors.get(model).unwrap_or(Color::DarkGray);
            let pct = if total == 0 {
                0.0
            } else {
                totals.total() as f64 / total as f64 * 100.0
            };
            let cost = app.pricing.lookup(model).map(|p| {
                cost_for(
                    p,
                    totals.input,
                    totals.output,
                    totals.cache_creation,
                    totals.cache_read,
                )
            });
            LegendRow {
                model,
                model_color: color,
                pct,
                input: totals.input,
                output: totals.output,
                cache_creation: totals.cache_creation,
                cache_read: totals.cache_read,
                cost,
            }
        })
        .collect();
    let table = build_table(&rows);
    frame.render_widget(table, area);
}

fn render_device_legend(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let total = app.device_aggregate.grand_total_tokens();
    let rows: Vec<DeviceRow> = app
        .device_aggregate
        .per_device
        .keys()
        .map(|device| {
            let totals = app.device_aggregate.totals(device);
            let color = app.device_colors.get(device).unwrap_or(Color::DarkGray);
            let pct = if total == 0 {
                0.0
            } else {
                totals.total() as f64 / total as f64 * 100.0
            };
            let cost = app.device_aggregate.cost(device, &app.pricing);
            DeviceRow {
                device,
                device_color: color,
                pct,
                input: totals.input,
                output: totals.output,
                cache_creation: totals.cache_creation,
                cache_read: totals.cache_read,
                cost,
            }
        })
        .collect();
    let table = build_device_table(&rows);
    frame.render_widget(table, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let status_text = footer_status_text(app);
    let status_style = match &app.footer {
        FooterStatus::Error(_) => Style::default().fg(Color::Red),
        FooterStatus::SyncingRemote | FooterStatus::SyncingPrices => {
            Style::default().fg(Color::Yellow)
        }
        _ => Style::default().fg(Color::DarkGray),
    };
    let dim = Style::default().add_modifier(Modifier::DIM);
    let chip_style = if app.verbose {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        dim
    };
    let line = Line::from(vec![
        Span::styled(status_text, status_style),
        Span::raw("  ·  "),
        Span::styled("q quit", dim),
        Span::raw("  ·  "),
        Span::styled("r refetch", dim),
        Span::raw("  ·  "),
        Span::styled(verbose_chip_text(app.verbose), chip_style),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn footer_status_text(app: &App) -> String {
    match &app.footer {
        FooterStatus::Idle => match app.status {
            Status::Bootstrap => "starting…".to_string(),
            Status::Active => "idle".to_string(),
            Status::Closed => "window closed · waiting for next session".to_string(),
        },
        FooterStatus::SyncingRemote => "syncing remote…".to_string(),
        FooterStatus::SyncingPrices => "syncing prices…".to_string(),
        FooterStatus::SyncedRemote { pulled, pushed } => {
            format!("synced remote · pulled {pulled} · pushed {pushed}")
        }
        FooterStatus::SyncedPrices { models } => {
            format!("synced prices · {models} models")
        }
        FooterStatus::Error(e) => format!("error: {e}"),
    }
}

fn verbose_chip_text(verbose: bool) -> &'static str {
    if verbose {
        "[v] verbose ✓"
    } else {
        "[v] verbose"
    }
}

/// Returns `(start, end)` column range (half-open, `end` exclusive) of the
/// `[v] verbose` chip rendered by [`render_footer`]. Used by the mouse
/// hit-test so we don't need to round-trip the rendered range through `App`.
/// `start == end` means the chip is off-screen (frame too narrow).
pub fn footer_verbose_chip_range(app: &App, frame_width: u16) -> (u16, u16) {
    let prefix_len =
        footer_status_text(app).chars().count() + "  ·  q quit  ·  r refetch  ·  ".chars().count();
    let chip = verbose_chip_text(app.verbose);
    let chip_len = chip.chars().count();
    let start = prefix_len.min(frame_width as usize) as u16;
    let end = (prefix_len + chip_len).min(frame_width as usize) as u16;
    (start, end)
}

/// Mirrors xclaude-usage.js logic: limit = used / (used_pct / 100). Falls back
/// to 0 when used_pct is non-positive.
fn output_limit(used: u64, used_pct: f64) -> u64 {
    if used_pct <= 0.0 {
        return 0;
    }
    ((used as f64) / (used_pct / 100.0)).round() as u64
}

fn eta_color(eta_seconds: Option<u64>, until_reset: i64) -> Color {
    match eta_seconds {
        None => Color::Gray,
        Some(_) if until_reset <= 0 => Color::DarkGray,
        Some(s) if (s as i64) >= until_reset => Color::Green,
        Some(s) if (s as i64) >= until_reset / 2 => Color::Yellow,
        Some(_) => Color::Red,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_limit_uses_pct_inversely() {
        // 50% used with 1000 tokens → limit = 2000
        assert_eq!(output_limit(1_000, 50.0), 2_000);
    }

    #[test]
    fn output_limit_zero_pct_returns_zero() {
        assert_eq!(output_limit(0, 0.0), 0);
        assert_eq!(output_limit(1_000, 0.0), 0);
    }

    #[test]
    fn output_limit_full_used() {
        assert_eq!(output_limit(700, 100.0), 700);
    }

    #[test]
    fn eta_color_idle_is_gray() {
        assert_eq!(eta_color(None, 1_000), Color::Gray);
    }

    #[test]
    fn eta_color_won_overflow_is_green() {
        assert_eq!(eta_color(Some(2_000), 1_000), Color::Green);
        assert_eq!(eta_color(Some(1_000), 1_000), Color::Green);
    }

    #[test]
    fn eta_color_tight_is_yellow() {
        // 600 < 1000 but >= 500 → yellow
        assert_eq!(eta_color(Some(600), 1_000), Color::Yellow);
    }

    #[test]
    fn eta_color_burning_fast_is_red() {
        // 100 < 500 → red
        assert_eq!(eta_color(Some(100), 1_000), Color::Red);
    }

    #[test]
    fn eta_color_after_reset_is_dark_gray() {
        assert_eq!(eta_color(Some(60), -10), Color::DarkGray);
    }

    #[test]
    fn tab_hit_first_half() {
        assert_eq!(tab_hit(0, 80), Some(WindowKind::FiveHour));
        assert_eq!(tab_hit(39, 80), Some(WindowKind::FiveHour));
    }

    #[test]
    fn tab_hit_second_half() {
        assert_eq!(tab_hit(40, 80), Some(WindowKind::SevenDay));
        assert_eq!(tab_hit(79, 80), Some(WindowKind::SevenDay));
    }

    #[test]
    fn tab_hit_past_width_is_none() {
        assert_eq!(tab_hit(80, 80), None);
        assert_eq!(tab_hit(100, 80), None);
    }

    #[test]
    fn tab_hit_zero_width_is_none() {
        assert_eq!(tab_hit(0, 0), None);
        assert_eq!(tab_hit(10, 0), None);
    }

    #[test]
    fn tab_hit_odd_width_splits_floor() {
        // width=81 → first half is cols 0..40 (40 cols), second half 40..81
        assert_eq!(tab_hit(39, 81), Some(WindowKind::FiveHour));
        assert_eq!(tab_hit(40, 81), Some(WindowKind::SevenDay));
    }
}
