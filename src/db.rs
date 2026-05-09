use std::path::Path;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use rusqlite::{Connection, OpenFlags};

pub fn open(path: &Path) -> Result<Connection> {
    if !path.exists() {
        return Err(eyre!(
            "SQLite database not found at {}. Make sure XClaudeUsage is installed and has run at least once.",
            path.display()
        ));
    }

    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    )?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(conn)
}

/// Returns the SQLite `data_version` pragma. Increments on any commit
/// from any connection (including other processes), so a cheap poll on this
/// value detects writes from the sibling XClaudeUsage process.
pub fn data_version(conn: &Connection) -> Result<i64> {
    let v: i64 = conn.pragma_query_value(None, "data_version", |r| r.get(0))?;
    Ok(v)
}
