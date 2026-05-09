use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use futures::StreamExt;
use tokio::time;

use crate::cli::Cli;
use crate::event::Action;
use crate::tui::Tui;
use crate::ui;

pub struct App {
    pub should_quit: bool,
    pub fetching: bool,
    pub tick_ms: u64,
    #[allow(dead_code)]
    pub args: Cli,
}

impl App {
    pub fn new(args: Cli) -> Self {
        Self {
            should_quit: false,
            fetching: false,
            tick_ms: args.tick_ms,
            args,
        }
    }

    pub async fn run(&mut self, terminal: &mut Tui) -> Result<()> {
        let mut events = EventStream::new();
        let mut ticker = time::interval(Duration::from_millis(self.tick_ms));

        terminal.draw(|f| ui::render(f, self))?;

        while !self.should_quit {
            let action = tokio::select! {
                _ = ticker.tick() => Action::Tick,
                maybe_event = events.next() => match maybe_event {
                    Some(Ok(ev)) => self.translate_event(ev),
                    Some(Err(e)) => return Err(e.into()),
                    None => Action::Quit,
                },
                _ = tokio::signal::ctrl_c() => Action::Quit,
            };

            self.update(action);

            terminal.draw(|f| ui::render(f, self))?;
        }
        Ok(())
    }

    fn translate_event(&self, ev: Event) -> Action {
        let Event::Key(key) = ev else {
            return Action::Noop;
        };
        if key.kind != KeyEventKind::Press {
            return Action::Noop;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Char('r') => Action::RemoteFetch,
            _ => Action::Noop,
        }
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Tick => { /* TODO: poll db::data_version, refresh state */ }
            Action::RemoteFetch => { /* TODO: spawn remote::sync_turso, set fetching */ }
            Action::Noop => {}
        }
    }
}
