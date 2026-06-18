use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, Gauge, Padding, Paragraph, Sparkline, SparklineBar};
use ratatui_bubbletea_components::{Help, KeyBinding, Spinner, SpinnerFrames};

use crate::app::{App, FooterStatus, IDLE_THRESHOLD_PER_MIN, RATE_WINDOW_MIN, Status};
use crate::format::compact;
use crate::pricing::cost_for;
use crate::widgets::header::HeaderState;
use crate::widgets::legend::{DeviceRow, LegendRow, build_device_table, build_table};
use crate::widgets::stacked_bar::StackedBar;
use crate::window::WindowKind;

/// Tabs row is always the first row of the frame.
pub const TABS_ROW: u16 = 0;

/// Minimum frame width before the header splits off a usage-gauge card on the
/// right. Below this the session line would get cramped, so the gauge is
/// dropped and the header takes the full width.
const GAUGE_MIN_WIDTH: u16 = 130;

/// Minimum frame width before the breakdown band shows a cost-by-type chart
/// beside the models card. Below this only the models card is shown.
const BREAKDOWN_SPLIT_WIDTH: u16 = 110;

/// Static key hints rendered in the footer via the `Help` component. The
/// `[v] verbose` toggle is intentionally NOT listed here — it stays a
/// hand-rendered, right-anchored, clickable chip (see [`render_footer`]).
const FOOTER_HINTS: [(&str, &str); 2] = [("q", "quit"), ("r", "refetch")];

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
    // Card grid: a 1-row tab strip, the header card, a breakdown card that
    // fills the remaining height (so the screen never reads as empty), and a
    // 1-row footer strip. `spacing(1)` gives the cards room to breathe.
    // TABS_ROW must stay row 0 and the footer must stay the last row so the
    // tab/chip mouse hit-tests keep matching `tab_hit` / `footer_verbose_chip_range`.
    let chunks = Layout::vertical([
        Constraint::Length(1), // tabs
        Constraint::Length(6), // header row (session + usage gauge)
        Constraint::Min(5),    // burn-rate sparkline — fills the vertical slack
        Constraint::Length(9), // breakdown band (models + tokens)
        Constraint::Length(1), // footer
    ])
    .spacing(1)
    .split(frame.area());

    render_tabs(frame, app, chunks[0]);

    // Header row: split off a usage-gauge card on the right when there's a
    // window and the terminal is wide enough to keep the session line intact.
    let header_area = chunks[1];
    if app.window.is_some() && header_area.width >= GAUGE_MIN_WIDTH {
        let hcols = Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)])
            .spacing(1)
            .split(header_area);
        render_header(frame, app, hcols[0]);
        render_usage_card(frame, app, hcols[1]);
    } else {
        render_header(frame, app, header_area);
    }

    render_burn_card(frame, app, chunks[2]);

    // Breakdown band: the models card, plus a token-composition chart beside it
    // when the terminal is wide enough.
    let breakdown = chunks[3];
    if breakdown.width >= BREAKDOWN_SPLIT_WIDTH {
        let bcols = Layout::horizontal([Constraint::Fill(2), Constraint::Fill(1)])
            .spacing(1)
            .split(breakdown);
        render_breakdown_card(frame, app, bcols[0]);
        render_cost_card(frame, app, bcols[1]);
    } else {
        render_breakdown_card(frame, app, breakdown);
    }

    render_footer(frame, app, chunks[4]);
}

fn render_tabs(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let theme = crate::style::bubble_theme();
    // Charm-style tabs: the active tab is accent + bold text (no heavy filled
    // background), the inactive one is muted. tab_hit stays a 50/50 width split.
    let active = theme.title;
    let inactive = theme.muted;
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

/// Renders a standard padded, rounded card titled `title` into `area` and
/// returns the inner content rect — the shared chrome for the dashboard panels.
fn card(frame: &mut Frame, area: ratatui::layout::Rect, title: &str) -> ratatui::layout::Rect {
    let block = crate::style::bubble_theme()
        .titled_block(title)
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// Share of `total` as a 0–100 percentage, guarding the empty window.
fn pct_of(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64 * 100.0
    }
}

fn render_header(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = view_title(app.view);
    let Some(window) = app.window else {
        let para = crate::style::bubble_theme()
            .paragraph_in_block("waiting for first hook from XClaudeUsage…", title);
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
    let eta_color = crate::style::eta_color(eta_seconds, until_reset);

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

/// The "hero" metric: a usage Gauge filling the header's right card when wide.
/// Fill level = window usage %; fill color = severity band (green/amber/red).
fn render_usage_card(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let theme = crate::style::bubble_theme();
    let inner = card(frame, area, " window usage ");

    let Some(window) = app.window else {
        frame.render_widget(Paragraph::new(theme.muted("—")), inner);
        return;
    };
    let used = app.aggregate.total_output;
    let limit = output_limit(used, window.used_percentage);
    let pct = window.used_percentage;
    // The bar length already encodes the ratio and the header repeats the
    // absolute figures, so the gauge label stays terse: just the percent.
    let label = if limit == 0 {
        "—".to_string()
    } else {
        format!("{pct:.0}%")
    };
    // `ratio` panics outside 0..=1. `used_percentage` comes from an external,
    // unvalidated SQLite column: it can exceed 100 (clamps fine) OR be NaN
    // (clamp passes NaN through → assert panic), so guard for finiteness.
    let ratio = {
        let r = pct / 100.0;
        if r.is_finite() { r.clamp(0.0, 1.0) } else { 0.0 }
    };
    // A closed (historical) window shows muted; an active one is severity-coded.
    let fill = if matches!(app.status, Status::Closed) {
        theme.palette.muted
    } else {
        crate::style::usage_severity(pct)
    };
    let gauge = Gauge::default()
        .ratio(ratio)
        .gauge_style(Style::default().fg(fill).bg(theme.palette.selected_background))
        .label(label)
        .use_unicode(true);
    // Center a 2-row gauge in the card so the accent severity color doesn't
    // flood the whole panel (restraint: one strong accent per view).
    let bar = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(2),
        Constraint::Fill(1),
    ])
    .split(inner)[1];
    frame.render_widget(gauge, bar);
}

/// A full-width burn-rate Sparkline (per-minute output) in a titled card.
/// Kept muted on purpose so it never competes with the accent/severity hero.
fn render_burn_card(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let theme = crate::style::bubble_theme();
    let block = theme
        .titled_block(" burn rate ")
        .padding(Padding::horizontal(1));
    // Right-aligned caption: current rate (last 15min), window average, and the
    // peak single-minute output. Adapts to whichever window (5h / 7d) is active.
    let block = if let Some(w) = app.window {
        let now = app.rate.rate_per_minute(app.now_secs, RATE_WINDOW_MIN);
        let peak = app.rate.peak_per_minute();
        let elapsed_min = ((app.now_secs - w.start_at) / 60).max(1) as f64;
        let avg = app.aggregate.total_output as f64 / elapsed_min;
        let now_val = now.round() as u64;
        // A non-empty series whose current 15-min rate rounds to 0 reads as
        // idle (distinct from the whole-series "no burn yet" empty state).
        let now_span = if now_val == 0 {
            theme.muted("idle")
        } else {
            theme.span(compact(now_val))
        };
        // Bottom-aligned so it reads as a caption under the chart.
        block.title_bottom(
            Line::from(vec![
                theme.muted("now "),
                now_span,
                theme.muted(" • avg "),
                theme.span(compact(avg.round() as u64)),
                theme.muted(" • peak "),
                theme.span(compact(peak)),
                theme.muted("/min "),
            ])
            .right_aligned(),
        )
    } else {
        block
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    // One bar per inner column; idle minutes are 0 so the width stays stable.
    let series = app.rate.minute_series(app.now_secs, inner.width as u32);
    if series.iter().all(|&v| v == 0) {
        frame.render_widget(Paragraph::new(theme.muted("no burn yet")), inner);
        return;
    }
    let peak = series.iter().copied().max().unwrap_or(1).max(1);
    let peak_col = series.iter().position(|&v| v == peak);
    // Heat-grade each column by its share of the peak (muted → foreground →
    // amber) so spikes read as "hot" pre-attentively. Height already encodes
    // magnitude, so the color is redundant reinforcement that degrades
    // gracefully on low-color terminals.
    let bars: Vec<SparklineBar> = series
        .iter()
        .map(|&v| {
            SparklineBar::from(v)
                .style(Style::default().fg(crate::style::heat_color(v as f64 / peak as f64)))
        })
        .collect();
    let sparkline = Sparkline::default().data(bars).max(peak);
    frame.render_widget(sparkline, inner);
    // Mark the peak column's top cell — anchors the "peak …/min" caption and
    // carries the "hot" signal independently of color (a11y / low-color safe).
    if let Some(i) = peak_col {
        if (i as u16) < inner.width {
            frame.buffer_mut().set_string(
                inner.x + i as u16,
                inner.y,
                "◆",
                Style::default().fg(theme.palette.warning),
            );
        }
    }
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

/// Frames the stacked bar + legend in a single rounded, padded card titled
/// ` models ` / ` devices `, so the lower ~70% of the screen reads as a
/// deliberate panel instead of a void. The inner widgets are rendered into the
/// block's padded inner rect; the `StackedBar` and legend `Table` themselves
/// are untouched (their per-series colors and geometry stay test-stable).
fn render_breakdown_card(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = if app.verbose { " devices " } else { " models " };
    let inner = card(frame, area, title);

    let rows = Layout::vertical([
        Constraint::Length(1), // stacked bar
        Constraint::Length(1), // breathing room
        Constraint::Min(0),    // legend table
    ])
    .split(inner);
    render_stacked_bar(frame, app, rows[0]);
    render_legend(frame, app, rows[2]);
}

/// Cost-by-type chart: one horizontal bar per token kind (input / output /
/// cache / read), but scaled by DOLLARS, not token count — `read` tokens are
/// far more numerous yet ~10× cheaper, so a count-based view is misleading.
/// Each token count is multiplied by its per-model, per-type price and summed
/// over the current view (models, or per-device→per-model in verbose).
/// The LARGEST cost bar gets the accent (the brightest color lands on the most
/// important value); `output` carries a thin `·` marker as the metered resource.
fn render_cost_card(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let theme = crate::style::bubble_theme();
    let inner = card(frame, area, " cost ");
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Accumulate per-type cost in dollars, using each model's own price.
    let (mut input, mut output, mut cache, mut read) = (0f64, 0f64, 0f64, 0f64);
    let mut add = |model: &str, t: &crate::aggregate::ModelTotals| {
        if let Some(p) = app.pricing.lookup(model) {
            input += t.input as f64 * p.input;
            output += t.output as f64 * p.output;
            cache += t.cache_creation as f64 * p.cache_creation;
            read += t.cache_read as f64 * p.cache_read;
        }
    };
    if app.verbose {
        for by_model in app.device_aggregate.per_device.values() {
            for (model, t) in by_model {
                add(model, t);
            }
        }
    } else {
        for (model, t) in &app.aggregate.per_model {
            add(model, t);
        }
    }

    if input + output + cache + read <= 0.0 {
        // No pricing loaded yet (or no usage) — don't draw empty $0 bars.
        frame.render_widget(Paragraph::new(theme.muted("no pricing yet")), inner);
        return;
    }

    // Bar values in cents (u64) to keep cent precision when scaling the bars.
    let cents = |d: f64| (d * 100.0).round().max(0.0) as u64;
    let kinds = [
        ("input", input),
        ("output", output),
        ("cache", cache),
        ("read", read),
    ];
    let maxv = cents(input.max(output).max(cache).max(read)).max(1);
    // First-max-wins, so exactly one bar is ever the loudest (accent budget = 1).
    let mut max_i = 0;
    for i in 1..kinds.len() {
        if kinds[i].1 > kinds[max_i].1 {
            max_i = i;
        }
    }
    let bars: Vec<Bar> = kinds
        .iter()
        .enumerate()
        .map(|(i, &(label, d))| {
            // output keeps a thin `·` marker as the metered resource.
            let text = if label == "output" {
                format!("${d:.2} ·")
            } else {
                format!("${d:.2}")
            };
            Bar::with_label(label, cents(d))
                .text_value(text)
                .style(if i == max_i { theme.accent } else { theme.muted })
        })
        .collect();
    let chart = BarChart::horizontal(bars)
        .max(maxv)
        .bar_width(1)
        .bar_gap(1)
        .label_style(theme.muted)
        .value_style(theme.text);
    frame.render_widget(chart, inner);
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
            let pct = pct_of(totals.total(), total);
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
            let pct = pct_of(totals.total(), total);
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
    let theme = crate::style::bubble_theme();
    let status_style = match &app.footer {
        FooterStatus::Error(_) => theme.error,
        FooterStatus::SyncingRemote | FooterStatus::SyncingPrices => theme.warning,
        _ => theme.muted,
    };

    // Right-anchored, fixed-width clickable chip. `footer_verbose_chip_range`
    // mirrors this exact geometry, so the mouse hit-test stays correct no matter
    // how long the status text or help hints to its left grow.
    let (chip_start, chip_end) = footer_verbose_chip_range(app.verbose, area.width);
    let chip_area = Rect {
        x: area.x + chip_start,
        y: area.y,
        width: chip_end - chip_start,
        height: area.height,
    };
    let chip_style = if app.verbose { theme.selected } else { theme.muted };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            verbose_chip_text(app.verbose),
            chip_style,
        ))),
        chip_area,
    );

    // Everything left of the chip: status text (left) + Help hints (right).
    let left = Rect {
        x: area.x,
        y: area.y,
        width: chip_start.saturating_sub(1),
        height: area.height,
    };
    let help = Help::new(FOOTER_HINTS.map(|(k, d)| KeyBinding::new(k, d))).theme(theme);
    let help_w = theme.help_line(FOOTER_HINTS).width() as u16;
    let cols = Layout::horizontal([Constraint::Min(1), Constraint::Length(help_w)]).split(left);

    // Left status segment: a short view tag + sync status, with the refresh
    // spinner animated inline while a background sync is in flight. The chip's
    // columns are reserved separately above, so this segment can grow freely
    // without shifting the clickable chip.
    let view_tag = match app.view {
        WindowKind::FiveHour => "5h",
        WindowKind::SevenDay => "7d",
    };
    let status_text = footer_status_text(app);
    if crate::app::spinner_active(app.fetching_remote, app.fetching_prices) {
        let frames = SpinnerFrames::DOTS;
        let mut spinner = Spinner::new()
            .frames(frames)
            .label(format!("{view_tag} • {status_text}"))
            .theme(theme);
        let len = frames.frames().len().max(1);
        for _ in 0..(app.spinner.frame_index() % len) {
            spinner.tick();
        }
        frame.render_widget(&spinner, cols[0]);
    } else {
        let line = Line::from(vec![
            Span::styled(view_tag, theme.accent),
            Span::styled(" • ", theme.muted),
            Span::styled(status_text, status_style),
        ]);
        frame.render_widget(Paragraph::new(line), cols[0]);
    }
    frame.render_widget(&help, cols[1]);
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

/// Returns the `(start, end)` column range (half-open, `end` exclusive) of the
/// clickable `[v] verbose` chip, which [`render_footer`] anchors to the right
/// edge of the footer with a fixed width. The mouse hit-test depends on this
/// matching the rendered chip geometry exactly.
pub fn footer_verbose_chip_range(verbose: bool, frame_width: u16) -> (u16, u16) {
    // Chip labels are all single-cell glyphs (including "✓"), so char count
    // equals display width. If a wide/zero-width glyph is ever added to the
    // chip, switch to a unicode-width measure so this stays aligned with render.
    let chip_w = verbose_chip_text(verbose).chars().count() as u16;
    (frame_width.saturating_sub(chip_w), frame_width)
}

/// Mirrors xclaude-usage.js logic: limit = used / (used_pct / 100). Falls back
/// to 0 when used_pct is non-positive.
fn output_limit(used: u64, used_pct: f64) -> u64 {
    if used_pct <= 0.0 {
        return 0;
    }
    ((used as f64) / (used_pct / 100.0)).round() as u64
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

    #[test]
    fn footer_chip_range_right_anchored_off() {
        // "[v] verbose" = 11 cols, anchored to the right edge of an 80-col footer.
        assert_eq!(footer_verbose_chip_range(false, 80), (69, 80));
    }

    #[test]
    fn footer_chip_range_right_anchored_on() {
        // "[v] verbose ✓" = 13 cols.
        assert_eq!(footer_verbose_chip_range(true, 80), (67, 80));
    }

    #[test]
    fn footer_chip_range_clamps_when_too_narrow() {
        // Frame narrower than the chip → start saturates to 0, never underflows.
        assert_eq!(footer_verbose_chip_range(false, 5), (0, 5));
    }
}
