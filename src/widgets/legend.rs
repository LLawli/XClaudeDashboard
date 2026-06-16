use ratatui::layout::{Alignment, Constraint};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Cell, Row, Table};

use crate::format::compact;

/// Inter-column spacing shared by the legend table and the total row.
pub const LEGEND_COLUMN_SPACING: u16 = 2;
/// Name-column width for the per-model table. Names are shortened (see
/// [`short_model`]) to `opus-4-8` / `haiku-4-5`, so this stays snug and the
/// metrics sit right beside the name.
pub const MODEL_NAME_W: u16 = 12;
/// Name-column width for the per-device table (slugs).
pub const DEVICE_NAME_W: u16 = 14;

/// Shared legend column widths; `name_w` is the fixed first (name) column. The
/// name column is fixed rather than greedy so the metrics line up next to the
/// names instead of being shoved to the far right edge.
pub fn legend_widths(name_w: u16) -> [Constraint; 7] {
    [
        Constraint::Length(name_w),
        Constraint::Length(7),  // %
        Constraint::Length(8),  // in
        Constraint::Length(8),  // out
        Constraint::Length(9),  // cache
        Constraint::Length(9),  // read
        Constraint::Length(10), // cost
    ]
}

/// A right-aligned cell, for clean tabular reading of numeric columns.
pub fn right_cell(s: impl Into<String>) -> Cell<'static> {
    Cell::from(Line::from(s.into()).alignment(Alignment::Right))
}

/// Compact display name for a model id: drops the `claude-` prefix and a
/// trailing `-YYYYMMDD`-style date (e.g. `claude-haiku-4-5-20251001` →
/// `haiku-4-5`, `claude-opus-4-8` → `opus-4-8`). Non-Claude ids pass through.
pub fn short_model(name: &str) -> String {
    let s = name.strip_prefix("claude-").unwrap_or(name);
    if let Some((head, tail)) = s.rsplit_once('-') {
        if tail.len() >= 6 && tail.bytes().all(|b| b.is_ascii_digit()) {
            return head.to_string();
        }
    }
    s.to_string()
}

pub struct LegendRow<'a> {
    pub model: &'a str,
    pub model_color: Color,
    pub pct: f64,
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub cost: Option<f64>,
}

pub fn build_table<'a>(rows: &'a [LegendRow<'a>]) -> Table<'a> {
    let header = Row::new(vec![
        Cell::from("model"),
        right_cell("%"),
        right_cell("in"),
        right_cell("out"),
        right_cell("cache"),
        right_cell("read"),
        right_cell("cost"),
    ])
    .style(crate::style::heading());

    let body: Vec<Row> = rows
        .iter()
        .map(|r| {
            let cost_text = match r.cost {
                Some(c) => format!("${c:>7.2}"),
                None => "—".into(),
            };
            Row::new(vec![
                Cell::from(short_model(r.model)).style(Style::default().fg(r.model_color)),
                right_cell(format!("{:.1}", r.pct)),
                right_cell(compact(r.input)),
                right_cell(compact(r.output)),
                right_cell(compact(r.cache_creation)),
                right_cell(compact(r.cache_read)),
                right_cell(cost_text),
            ])
        })
        .collect();

    Table::new(body, legend_widths(MODEL_NAME_W))
        .header(header)
        .column_spacing(LEGEND_COLUMN_SPACING)
}

pub struct DeviceRow<'a> {
    pub device: &'a str,
    pub device_color: Color,
    pub pct: f64,
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub cost: Option<f64>,
}

pub fn build_device_table<'a>(rows: &'a [DeviceRow<'a>]) -> Table<'a> {
    let header = Row::new(vec![
        Cell::from("device"),
        right_cell("%"),
        right_cell("in"),
        right_cell("out"),
        right_cell("cache"),
        right_cell("read"),
        right_cell("cost"),
    ])
    .style(crate::style::heading());

    let body: Vec<Row> = rows
        .iter()
        .map(|r| {
            let cost_text = match r.cost {
                Some(c) => format!("${c:>7.2}"),
                None => "—".into(),
            };
            Row::new(vec![
                Cell::from(short_model(r.device)).style(Style::default().fg(r.device_color)),
                right_cell(format!("{:.1}", r.pct)),
                right_cell(compact(r.input)),
                right_cell(compact(r.output)),
                right_cell(compact(r.cache_creation)),
                right_cell(compact(r.cache_read)),
                right_cell(cost_text),
            ])
        })
        .collect();

    Table::new(body, legend_widths(DEVICE_NAME_W))
        .header(header)
        .column_spacing(LEGEND_COLUMN_SPACING)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

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

    #[test]
    fn renders_header_row_with_column_names() {
        let rows: Vec<LegendRow> = vec![];
        let table = build_table(&rows);
        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        Widget::render(table, area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("model"));
        assert!(text.contains("in"));
        assert!(text.contains("out"));
        assert!(text.contains("cache"));
        assert!(text.contains("read"));
        assert!(text.contains("cost"));
    }

    #[test]
    fn renders_model_in_assigned_color() {
        let rows = [LegendRow {
            model: "claude-opus-4-7",
            model_color: Color::Cyan,
            pct: 62.1,
            input: 12_000,
            output: 192_000,
            cache_creation: 3_000,
            cache_read: 67_000,
            cost: Some(14.32),
        }];
        let table = build_table(&rows);
        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        Widget::render(table, area, &mut buf);
        // Find the row containing "claude-opus" — its fg should be Cyan
        let mut found_cyan = false;
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].fg == Color::Cyan {
                    found_cyan = true;
                    break;
                }
            }
        }
        assert!(found_cyan, "expected at least one Cyan cell for opus");
    }

    #[test]
    fn renders_cost_with_dollar_sign_when_present() {
        let rows = [LegendRow {
            model: "opus",
            model_color: Color::Cyan,
            pct: 50.0,
            input: 1_000,
            output: 2_000,
            cache_creation: 500,
            cache_read: 1_500,
            cost: Some(1.23),
        }];
        let table = build_table(&rows);
        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        Widget::render(table, area, &mut buf);
        assert!(buffer_text(&buf).contains("$"));
    }

    #[test]
    fn device_table_renders_header() {
        let rows: Vec<DeviceRow> = vec![];
        let table = build_device_table(&rows);
        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        Widget::render(table, area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("device"));
        assert!(text.contains("cost"));
    }

    #[test]
    fn device_table_renders_local_row_with_color() {
        let rows = [DeviceRow {
            device: "local",
            device_color: Color::Cyan,
            pct: 62.1,
            input: 12_000,
            output: 192_000,
            cache_creation: 3_000,
            cache_read: 67_000,
            cost: Some(14.32),
        }];
        let table = build_device_table(&rows);
        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        Widget::render(table, area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("local"));
        assert!(text.contains("$"));
        // confirm device label cell is colorized
        let mut found_cyan = false;
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].fg == Color::Cyan {
                    found_cyan = true;
                    break;
                }
            }
        }
        assert!(found_cyan);
    }

    #[test]
    fn device_table_renders_dash_when_cost_missing() {
        let rows = [DeviceRow {
            device: "luka-notebook",
            device_color: Color::Green,
            pct: 10.0,
            input: 1,
            output: 1,
            cache_creation: 0,
            cache_read: 0,
            cost: None,
        }];
        let table = build_device_table(&rows);
        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        Widget::render(table, area, &mut buf);
        let text = buffer_text(&buf);
        assert!(text.contains("—"));
    }

    #[test]
    fn renders_dash_for_missing_cost() {
        let rows = [LegendRow {
            model: "opus",
            model_color: Color::Cyan,
            pct: 50.0,
            input: 1_000,
            output: 2_000,
            cache_creation: 0,
            cache_read: 0,
            cost: None,
        }];
        let table = build_table(&rows);
        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        Widget::render(table, area, &mut buf);
        let text = buffer_text(&buf);
        // Em-dash UTF-8 is "—". When cost is None we render that literal.
        assert!(
            text.contains("—"),
            "expected em-dash for missing cost; got: {text:?}"
        );
    }
}
