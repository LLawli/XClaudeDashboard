use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Padding, Paragraph, Row, Sparkline, Table};
use ratatui_bubbletea_components::{Help, KeyBinding, SpinnerFrames};

use crate::app::{App, FooterStatus, IDLE_THRESHOLD_PER_MIN, RATE_WINDOW_MIN, Status};
use crate::format::compact;
use crate::pricing::cost_for;
use crate::style::{bubble_theme, severity_color};
use crate::widgets::header::HeaderState;
use crate::widgets::legend::{DeviceRow, LegendRow, build_device_table, build_table, short_model};
use crate::widgets::stacked_bar::StackedBar;
use crate::window::WindowKind;

/// Tabs row is always the first row of the (content) frame.
pub const TABS_ROW: u16 = 0;

/// Static key hints rendered in the footer via the `Help` component. The
/// `[v] verbose` toggle is intentionally NOT listed here — it stays a
/// hand-rendered, right-anchored, clickable chip (see [`render_footer`]).
const FOOTER_HINTS: [(&str, &str); 2] = [("q", "quit"), ("r", "refetch")];

const HERO_H: u16 = 6;
const BREAKDOWN_H: u16 = 4;
/// Capped height for the burn-rate card (kept calm, not screen-dominating).
const BURN_H: u16 = 16;
/// Minutes of history shown in the burn-rate sparkline.
const BURN_WINDOW_MIN: u32 = 60;

/// Hybrid content column: full-bleed at `<= 120` cols, clamped to a centered
/// ~110-col column on wider terminals so the dashboard reads as a deliberate
/// composition rather than top-left text floating in a void. Pure so the
/// renderer and the mouse hit-test agree on the geometry.
pub fn content_rect(area: Rect) -> Rect {
    const MAX_W: u16 = 110;
    if area.width > 120 {
        let pad = area.width.saturating_sub(MAX_W) / 2;
        Rect {
            x: area.x + pad,
            y: area.y,
            width: MAX_W,
            height: area.height,
        }
    } else {
        area
    }
}

/// Hit-test for a click on the tabs row (in content-column space). Each tab
/// claims half the width; clicks past `width` (or `width == 0`) return `None`.
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

/// A rounded, padded section card with a neutral-bold title (accent pink stays
/// reserved for the active tab + the verbose chip).
fn section_block<'a>(title: &'a str) -> Block<'a> {
    bubble_theme()
        .titled_block(title)
        .title_style(crate::style::heading())
        .padding(Padding::horizontal(1))
}

#[derive(Clone, Copy)]
enum Slot {
    Tabs,
    Gap,
    Hero,
    Breakdown,
    BurnRate,
    Legend,
    Flex,
}

pub fn render(frame: &mut Frame, app: &App) {
    let content = content_rect(frame.area());

    // No window yet → calm composed waiting screen.
    if app.window.is_none() {
        render_empty(frame, app, content);
        return;
    }

    // Bottom-anchor the footer (with a 1-row gap above it) so the footer is
    // ALWAYS on the last row — matching the mouse hit-test — even when the body
    // is squeezed on a short terminal.
    let outer = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(content);
    let body = outer[0];
    render_footer(frame, app, outer[2]);

    // Burn-rate history; only surface the card when there is enough signal and
    // enough vertical room (it is the first thing to drop on short terminals).
    let series = app.rate.per_minute_series(app.now_secs, BURN_WINDOW_MIN);
    let nonempty = series.iter().filter(|&&v| v > 0).count();
    let show_spark = body.height >= 34 && nonempty >= 3;

    let legend_h = legend_height(app);

    // FlexTop + FlexBot center the card stack between the pinned tabs (row 0)
    // and the bottom-anchored footer, so a sparse dashboard reads as a calm,
    // composed column rather than a top-heavy block or a screen-tall chart.
    let mut slots: Vec<(Slot, Constraint)> = vec![
        (Slot::Tabs, Constraint::Length(1)),
        (Slot::Gap, Constraint::Length(1)),
        (Slot::Flex, Constraint::Min(0)),
        (Slot::Hero, Constraint::Length(HERO_H)),
        (Slot::Gap, Constraint::Length(1)),
        (Slot::Breakdown, Constraint::Length(BREAKDOWN_H)),
        (Slot::Gap, Constraint::Length(1)),
    ];
    if show_spark {
        slots.push((Slot::BurnRate, Constraint::Length(BURN_H)));
        slots.push((Slot::Gap, Constraint::Length(1)));
    }
    slots.push((Slot::Legend, Constraint::Length(legend_h)));
    slots.push((Slot::Flex, Constraint::Min(0)));

    let cons: Vec<Constraint> = slots.iter().map(|(_, c)| *c).collect();
    let rects = Layout::vertical(cons).split(body);

    for (i, (slot, _)) in slots.iter().enumerate() {
        let area = rects[i];
        match slot {
            Slot::Tabs => render_tabs(frame, app, area),
            Slot::Hero => render_hero(frame, app, area),
            Slot::Breakdown => render_breakdown(frame, app, area),
            Slot::BurnRate => render_burnrate(frame, area, &series),
            Slot::Legend => render_legend(frame, app, area),
            Slot::Gap | Slot::Flex => {}
        }
    }
}

fn render_empty(frame: &mut Frame, app: &App, content: Rect) {
    let outer = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(content);
    render_footer(frame, app, outer[2]);

    let rects = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
    ])
    .split(outer[0]);

    render_tabs(frame, app, rects[0]);

    let block = section_block(view_title(app.view));
    let inner = block.inner(rects[2]);
    frame.render_widget(block, rects[2]);
    render_centered_note(frame, inner, "waiting for first hook from XClaudeUsage…");
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let halves = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    let theme = bubble_theme();
    // Charm-style tabs: active tab is accent + bold text (no heavy filled
    // background), inactive is muted. tab_hit stays a 50/50 width split.
    let active = theme.title;
    let inactive = theme.muted;
    let (style_5h, style_7d) = match app.view {
        WindowKind::FiveHour => (active, inactive),
        WindowKind::SevenDay => (inactive, active),
    };

    frame.render_widget(
        Paragraph::new("5h (h)").alignment(Alignment::Center).style(style_5h),
        halves[0],
    );
    frame.render_widget(
        Paragraph::new("7d (s)").alignment(Alignment::Center).style(style_7d),
        halves[1],
    );
}

fn render_hero(frame: &mut Frame, app: &App, area: Rect) {
    let Some(window) = app.window else {
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
    let severity = severity_color(eta_seconds, until_reset);

    let spinner_note = if crate::app::spinner_active(app.fetching_remote, app.fetching_prices) {
        let frames = SpinnerFrames::DOTS;
        let glyph = frames.frames()[app.spinner.frame_index() % frames.frames().len().max(1)];
        Some(format!(" {glyph} {} ", footer_status_text(app)))
    } else {
        None
    };

    let state = HeaderState {
        title: view_title(app.view),
        show_date: matches!(app.view, WindowKind::SevenDay),
        started: window.start_at,
        resets_at: window.resets_at,
        now: app.now_secs,
        used_pct: window.used_percentage,
        output_used,
        output_limit,
        eta_seconds,
        eta_color: severity,
        rate_per_min: app.rate.rate_per_minute(app.now_secs, RATE_WINDOW_MIN),
        rate_window_min: RATE_WINDOW_MIN,
        spinner_note,
    };
    frame.render_widget(&state, area);
}

fn view_title(view: WindowKind) -> &'static str {
    match view {
        WindowKind::FiveHour => " session 5h ",
        WindowKind::SevenDay => " session 7d ",
    }
}

/// `(color, fraction, name)` for each breakdown slice (per model, or per device
/// in verbose mode). Empty when the window has no usage.
fn breakdown_slices(app: &App) -> Vec<(Color, f64, String)> {
    if app.verbose {
        let total = app.device_aggregate.grand_total_tokens();
        if total == 0 {
            return Vec::new();
        }
        app.device_aggregate
            .per_device
            .keys()
            .map(|device| {
                let color = app.device_colors.get(device).unwrap_or(Color::DarkGray);
                let frac = app.device_aggregate.totals(device).total() as f64 / total as f64;
                (color, frac, device.clone())
            })
            .collect()
    } else {
        let total: u64 = app.aggregate.per_model.values().map(|t| t.total()).sum();
        if total == 0 {
            return Vec::new();
        }
        app.aggregate
            .per_model
            .iter()
            .map(|(model, totals)| {
                let color = app.colors.get(model).unwrap_or(Color::DarkGray);
                (color, totals.total() as f64 / total as f64, model.clone())
            })
            .collect()
    }
}

fn render_breakdown(frame: &mut Frame, app: &App, area: Rect) {
    let theme = bubble_theme();
    let block = section_block(" breakdown ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let slices = breakdown_slices(app);
    if slices.is_empty() {
        render_centered_note(frame, inner, "no usage in this window yet");
        return;
    }

    // inner is 2 rows: the proportion bar, then a colored chip legend.
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
    let bar_row = rows[0];

    if let [(color, _frac, name)] = slices.as_slice() {
        // A single series is always 100% — label it inline; no chip row needed.
        let short = short_model(name);
        let name_w = (short.chars().count() as u16 + 1).min(bar_row.width);
        let cols = Layout::horizontal([
            Constraint::Length(name_w),
            Constraint::Min(4),
            Constraint::Length(8),
        ])
        .split(bar_row);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(short, Style::new().fg(*color)))),
            cols[0],
        );
        frame.render_widget(StackedBar { slices: &[(*color, 1.0)] }, cols[1]);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("100.0%", theme.muted))).alignment(Alignment::Right),
            cols[2],
        );
    } else {
        let sb: Vec<(Color, f64)> = slices.iter().map(|(c, f, _)| (*c, *f)).collect();
        frame.render_widget(StackedBar { slices: &sb }, bar_row);
        // Colored chip legend so the bar's segments are actually readable.
        let chips: Vec<Span> = slices
            .iter()
            .flat_map(|(c, f, name)| {
                [
                    Span::styled("● ", Style::new().fg(*c)),
                    Span::styled(format!("{} {:.0}%   ", short_model(name), f * 100.0), theme.muted),
                ]
            })
            .collect();
        frame.render_widget(Paragraph::new(Line::from(chips)), rows[1]);
    }
}

fn render_burnrate(frame: &mut Frame, area: Rect, series: &[u64]) {
    let theme = bubble_theme();
    let annotation = Span::styled(format!(" tokens/min · last {BURN_WINDOW_MIN}m "), theme.muted);
    let block =
        section_block(" burn rate ").title_top(Line::from(annotation).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 10 || inner.height == 0 {
        return;
    }

    let cols = Layout::horizontal([Constraint::Min(8), Constraint::Length(12)]).split(inner);
    let now = series.last().copied().unwrap_or(0);
    let peak = series.iter().copied().max().unwrap_or(0);

    // Calm informational blue (not the loud accent) so the chart reads as
    // context, with the gauge staying the hero.
    let spark = Sparkline::default()
        .data(series.to_vec())
        .style(Style::new().fg(theme.palette.focused_border));
    frame.render_widget(spark, cols[0]);

    let caps = Paragraph::new(vec![
        Line::from(Span::styled(format!("now  {}", compact(now)), theme.muted)),
        Line::from(Span::styled(format!("peak {}", compact(peak)), theme.muted)),
    ]);
    frame.render_widget(caps, cols[1]);
}

fn render_legend(frame: &mut Frame, app: &App, area: Rect) {
    use crate::widgets::legend::{
        DEVICE_NAME_W, LEGEND_COLUMN_SPACING, MODEL_NAME_W, legend_widths, right_cell,
    };
    let theme = bubble_theme();
    let (title, name_w) = if app.verbose {
        (" devices ", DEVICE_NAME_W)
    } else {
        (" models ", MODEL_NAME_W)
    };
    let block = section_block(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    // Left-aligned (names start at the card's left padding, consistent with the
    // breakdown chips above); the compact table left-packs and any leftover
    // width is plain right margin.
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let total = if app.verbose {
        render_device_legend(frame, app, parts[0])
    } else {
        render_model_legend(frame, app, parts[0])
    };

    if let Some(t) = total {
        // Same column widths as the table above so "total" + cost align under
        // the read/cost columns.
        let row = Row::new(vec![
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
            right_cell("total").style(theme.muted),
            right_cell(format!("${t:>7.2}")).style(theme.text),
        ]);
        frame.render_widget(
            Table::new([row], legend_widths(name_w)).column_spacing(LEGEND_COLUMN_SPACING),
            parts[1],
        );
    }
}

/// Renders the model table into `area`, returning the summed cost of models
/// with a known price (`None` when no costs are known / no rows).
fn render_model_legend(frame: &mut Frame, app: &App, area: Rect) -> Option<f64> {
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

    if rows.is_empty() {
        render_centered_note(frame, area, "collecting usage…");
        return None;
    }
    let any_cost = rows.iter().any(|r| r.cost.is_some());
    let total_cost: f64 = rows.iter().filter_map(|r| r.cost).sum();
    frame.render_widget(build_table(&rows), area);
    any_cost.then_some(total_cost)
}

fn render_device_legend(frame: &mut Frame, app: &App, area: Rect) -> Option<f64> {
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

    if rows.is_empty() {
        render_centered_note(frame, area, "collecting usage…");
        return None;
    }
    let any_cost = rows.iter().any(|r| r.cost.is_some());
    let total_cost: f64 = rows.iter().filter_map(|r| r.cost).sum();
    frame.render_widget(build_device_table(&rows), area);
    any_cost.then_some(total_cost)
}

/// Vertical height of the legend card: 2 border + 1 header + N rows + 1 total.
fn legend_height(app: &App) -> u16 {
    let n = if app.verbose {
        app.device_aggregate.per_device.len()
    } else {
        app.aggregate.per_model.len()
    };
    (n as u16).clamp(1, 15) + 4
}

fn render_centered_note(frame: &mut Frame, area: Rect, msg: &str) {
    if area.height == 0 {
        return;
    }
    let theme = bubble_theme();
    let row = Rect {
        x: area.x,
        y: area.y + area.height / 2,
        width: area.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(msg.to_string(), theme.muted))).alignment(Alignment::Center),
        row,
    );
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let theme = bubble_theme();
    let status_style = match &app.footer {
        FooterStatus::Error(_) => theme.error,
        FooterStatus::SyncingRemote | FooterStatus::SyncingPrices => theme.warning,
        _ => theme.muted,
    };

    // Right-anchored, fixed-width clickable chip. `footer_verbose_chip_range`
    // mirrors this exact geometry (content-relative), so the mouse hit-test
    // stays correct no matter how long the status/help to its left grow.
    let (chip_start, chip_end) = footer_verbose_chip_range(app.verbose, area.width);
    let chip_area = Rect {
        x: area.x + chip_start,
        y: area.y,
        width: chip_end - chip_start,
        height: area.height,
    };
    let chip_style = if app.verbose { theme.selected } else { theme.muted };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(verbose_chip_text(app.verbose), chip_style))),
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
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(footer_status_text(app), status_style))),
        cols[0],
    );
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

/// Returns the `(start, end)` column range (half-open, content-relative) of the
/// clickable `[v] verbose` chip, which [`render_footer`] anchors to the right
/// edge of the footer column with a fixed width. The mouse hit-test depends on
/// this matching the rendered chip geometry exactly.
pub fn footer_verbose_chip_range(verbose: bool, frame_width: u16) -> (u16, u16) {
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
    fn content_rect_full_bleed_when_narrow() {
        let a = Rect::new(0, 0, 100, 40);
        assert_eq!(content_rect(a), a);
        let a = Rect::new(0, 0, 120, 40);
        assert_eq!(content_rect(a), a);
    }

    #[test]
    fn content_rect_centers_when_wide() {
        let c = content_rect(Rect::new(0, 0, 190, 55));
        assert_eq!(c.width, 110);
        assert_eq!(c.x, (190 - 110) / 2);
        assert_eq!(c.height, 55);
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
        assert_eq!(footer_verbose_chip_range(false, 5), (0, 5));
    }

    /// Regression: the mouse hit-test keys off row `height - 1`, so the footer
    /// must actually render there even when the body is squeezed on a short
    /// terminal (the footer is bottom-anchored in `render`).
    #[test]
    fn footer_pins_to_last_row_on_short_terminals() {
        use crate::app::{App, Status};
        use crate::cli::Cli;
        use crate::window::Window;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let cli = Cli {
            db_path: Some(std::path::PathBuf::from("/nonexistent-xclaude.db")),
            cloud_config: None,
            tick_ms: 200,
        };
        // Skip gracefully in environments where App::new can't resolve paths.
        let Ok(mut app) = App::new(cli) else {
            return;
        };
        let now = app.now_secs;
        app.status = Status::Active;
        app.window = Some(Window {
            start_at: now - 9_600,
            resets_at: now + 8_400,
            used_percentage: 15.0,
            updated_at: now,
        });

        for h in [5u16, 6, 8, 10, 24] {
            let mut term = Terminal::new(TestBackend::new(80, h)).unwrap();
            term.draw(|f| render(f, &app)).unwrap();
            let buf = term.backend().buffer();
            let last: String = (0..80).map(|x| buf[(x, h - 1)].symbol()).collect();
            assert!(
                last.contains("verbose"),
                "footer/[v] chip not on last row at height {h}: {last:?}"
            );
        }
    }

    /// Visual smoke test: builds sample state, renders the whole UI to a
    /// TestBackend and prints the buffer. Run with:
    ///   cargo test ui::tests::preview_dump -- --ignored --nocapture
    #[test]
    #[ignore = "visual preview; run with --ignored --nocapture"]
    fn preview_dump() {
        use crate::aggregate::ModelTotals;
        use crate::app::{App, Status};
        use crate::cli::Cli;
        use crate::window::Window;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let cli = Cli {
            db_path: Some(std::path::PathBuf::from("/nonexistent-xclaude.db")),
            cloud_config: None,
            tick_ms: 200,
        };
        let mut app = App::new(cli).expect("app");
        let now = app.now_secs;
        app.status = Status::Active;
        app.window = Some(Window {
            start_at: now - 9_600,
            resets_at: now + 8_400,
            used_percentage: 15.0,
            updated_at: now,
        });
        app.aggregate.total_output = 1_100_000;
        app.aggregate.per_model.insert(
            "claude-opus-4-8".to_string(),
            ModelTotals {
                input: 1_100_000,
                output: 1_100_000,
                cache_creation: 9_800_000,
                cache_read: 72_000_000,
            },
        );
        app.colors.assign("claude-opus-4-8");
        app.aggregate.per_model.insert(
            "claude-haiku-4-5-20251001".to_string(),
            ModelTotals {
                input: 12_000,
                output: 38_000,
                cache_creation: 416_000,
                cache_read: 5_100_000,
            },
        );
        app.colors.assign("claude-haiku-4-5-20251001");
        let samples: Vec<(i64, u64)> = (0..60)
            .map(|m| (now - (60 - m) * 60, ((m as u64 * 37) % 40 + 5) * 1_000))
            .collect();
        app.rate.replace_from_samples(samples);

        let dump = |label: &str, w: u16, h: u16, app: &App| {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| render(f, app)).unwrap();
            let buf = term.backend().buffer();
            let mut out = format!("\n===== {label} ({w}x{h}) =====\n");
            for y in 0..buf.area.height {
                for x in 0..buf.area.width {
                    out.push_str(buf[(x, y)].symbol());
                }
                out.push('\n');
            }
            println!("{out}");
        };

        dump("wide", 190, 55, &app);
        dump("narrow", 80, 24, &app);

        // Empty / waiting state must not panic and must look composed.
        let mut empty = App::new(Cli {
            db_path: Some(std::path::PathBuf::from("/nonexistent-xclaude.db")),
            cloud_config: None,
            tick_ms: 200,
        })
        .expect("app");
        empty.window = None;
        dump("empty", 120, 20, &empty);
    }
}
