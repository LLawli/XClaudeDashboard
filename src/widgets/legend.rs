use ratatui::layout::Constraint;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Cell, Row, Table};

use crate::format::compact;

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
        Cell::from("%"),
        Cell::from("in"),
        Cell::from("out"),
        Cell::from("cache"),
        Cell::from("read"),
        Cell::from("cost"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let body: Vec<Row> = rows
        .iter()
        .map(|r| {
            let cost_text = match r.cost {
                Some(c) => format!("${c:>7.2}"),
                None => "—".into(),
            };
            Row::new(vec![
                Cell::from(r.model).style(Style::default().fg(r.model_color)),
                Cell::from(format!("{:>5.1}", r.pct)),
                Cell::from(compact(r.input)),
                Cell::from(compact(r.output)),
                Cell::from(compact(r.cache_creation)),
                Cell::from(compact(r.cache_read)),
                Cell::from(cost_text),
            ])
        })
        .collect();

    Table::new(
        body,
        [
            Constraint::Min(20),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(8),
        ],
    )
    .header(header)
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
        Cell::from("%"),
        Cell::from("in"),
        Cell::from("out"),
        Cell::from("cache"),
        Cell::from("read"),
        Cell::from("cost"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let body: Vec<Row> = rows
        .iter()
        .map(|r| {
            let cost_text = match r.cost {
                Some(c) => format!("${c:>7.2}"),
                None => "—".into(),
            };
            Row::new(vec![
                Cell::from(r.device).style(Style::default().fg(r.device_color)),
                Cell::from(format!("{:>5.1}", r.pct)),
                Cell::from(compact(r.input)),
                Cell::from(compact(r.output)),
                Cell::from(compact(r.cache_creation)),
                Cell::from(compact(r.cache_read)),
                Cell::from(cost_text),
            ])
        })
        .collect();

    Table::new(
        body,
        [
            Constraint::Min(20),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(8),
        ],
    )
    .header(header)
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
