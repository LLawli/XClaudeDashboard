use std::path::Path;

use color_eyre::Result;

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct SyncOutcome {
    pub pulled_rows: usize,
    pub pushed_rows: usize,
}

/// Pushes pending deltas and pulls remote rows from Turso, writing them back
/// to the same SQLite that XClaudeUsage uses (tables `cloud_cache` and
/// `cloud_state`). Mirrors `syncCloud()` in
/// `~/Projeto/XClaudeUsage/xclaude-record.js:240-329`.
#[allow(dead_code)]
pub async fn sync_turso(_db_path: &Path, _cloud_config: &serde_json::Value) -> Result<SyncOutcome> {
    unimplemented!("sync_turso: port syncCloud() from xclaude-record.js")
}
