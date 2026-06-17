mod aggregate;
mod app;
mod cli;
mod colors;
pub mod config;
pub mod db;
mod event;
mod format;
mod pricing;
mod rate;
mod remote;
mod style;
mod tui;
mod ui;
mod widgets;
mod window;

use clap::Parser;
use color_eyre::Result;

pub fn run() -> Result<()> {
    // Cache the local UTC offset BEFORE the multi-thread runtime spawns —
    // `time::UtcOffset::current_local_offset` is unsound once other threads exist.
    let _ = format::init_local_offset();

    let args = cli::Cli::parse();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let mut terminal = tui::init()?;
        let result = match app::App::new(args) {
            Ok(mut app) => app.run(&mut terminal).await,
            Err(e) => Err(e),
        };
        let _ = tui::restore();
        result
    })
}
