use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, FooterStatus, IDLE_THRESHOLD_PER_MIN, RATE_WINDOW_MIN, Status};
use crate::pricing::cost_for;
use crate::widgets::header::HeaderState;
use crate::widgets::legend::{LegendRow, build_table};
use crate::widgets::stacked_bar::StackedBar;

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // header block (4 lines + 2 borders)
            Constraint::Length(1), // stacked bar
            Constraint::Length(1), // spacer
            Constraint::Min(5),    // legend
            Constraint::Length(1), // footer
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    render_stacked_bar(frame, app, chunks[1]);
    render_legend(frame, app, chunks[3]);
    render_footer(frame, app, chunks[4]);
}

fn render_header(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(window) = app.window else {
        let block = Block::bordered().title(" session 5h ");
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

fn render_stacked_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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

fn render_footer(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let status_text = match &app.footer {
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
    };
    let status_style = match &app.footer {
        FooterStatus::Error(_) => Style::default().fg(Color::Red),
        FooterStatus::SyncingRemote | FooterStatus::SyncingPrices => {
            Style::default().fg(Color::Yellow)
        }
        _ => Style::default().fg(Color::DarkGray),
    };
    let line = Line::from(vec![
        Span::styled(status_text, status_style),
        Span::raw("  ·  "),
        Span::styled("q quit", Style::default().add_modifier(Modifier::DIM)),
        Span::raw("  ·  "),
        Span::styled("r refetch", Style::default().add_modifier(Modifier::DIM)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
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
}
