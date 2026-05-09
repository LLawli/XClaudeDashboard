use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Paragraph};

use crate::app::App;

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let header = Paragraph::new(" xclaude ").style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(header, chunks[0]);

    let body = Block::bordered().title(" dashboard ");
    frame.render_widget(body, chunks[1]);

    let status = if app.fetching {
        "syncing… · q quit · r refetch"
    } else {
        "idle · q quit · r refetch"
    };
    frame.render_widget(Paragraph::new(status), chunks[2]);
}
