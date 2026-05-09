mod app;
mod cli;
pub mod config;
pub mod db;
mod event;
mod remote;
mod tui;
mod ui;

use clap::Parser;
use color_eyre::Result;

pub fn run() -> Result<()> {
    let args = cli::Cli::parse();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let mut terminal = tui::init()?;
        let result = app::App::new(args).run(&mut terminal).await;
        let _ = tui::restore();
        result
    })
}
