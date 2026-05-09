use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "xclaude",
    version,
    about = "Real-time TUI dashboard for XClaudeUsage"
)]
pub struct Cli {
    /// Path to the SQLite database written by XClaudeUsage.
    /// Defaults to ~/.claude/data/xclaude-usage.db.
    #[arg(long, env = "XCLAUDE_DB")]
    pub db_path: Option<PathBuf>,

    /// Path to xclaude-cloud.json (Turso credentials).
    /// Defaults to ~/.claude/data/xclaude-cloud.json.
    #[arg(long, env = "XCLAUDE_CLOUD_CONFIG")]
    pub cloud_config: Option<PathBuf>,

    /// Tick interval in milliseconds for DB change polling.
    #[arg(long, default_value_t = 200)]
    pub tick_ms: u64,
}
