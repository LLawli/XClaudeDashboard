use color_eyre::Result;
use rusqlite::{Connection, OptionalExtension};

/// Snapshot of the active 5h session window. Read from the `five_hour_window`
/// table written by `xclaude-usage.js` (sibling project XClaudeUsage). All
/// timestamps are **epoch seconds** (singleton row, PK = `id = 1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub start_at: i64,
    pub resets_at: i64,
    pub used_percentage: f64,
    pub updated_at: i64,
}

impl Window {
    /// Reads the singleton row. Returns `Ok(None)` if the row hasn't been
    /// written yet (e.g. XClaudeUsage hasn't fired the first hook of the
    /// new schema). Returns `Err` only on real SQLite errors.
    pub fn current(conn: &Connection) -> Result<Option<Self>> {
        let row = conn
            .query_row(
                "SELECT start_at, resets_at, used_percentage, updated_at \
                 FROM five_hour_window WHERE id = 1",
                [],
                |r| {
                    Ok(Self {
                        start_at: r.get(0)?,
                        resets_at: r.get(1)?,
                        used_percentage: r.get(2)?,
                        updated_at: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    #[allow(dead_code)]
    pub fn is_active(&self, now_secs: i64) -> bool {
        now_secs < self.resets_at
    }

    #[allow(dead_code)]
    pub fn seconds_until_reset(&self, now_secs: i64) -> i64 {
        self.resets_at - now_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE five_hour_window (
                id              INTEGER PRIMARY KEY CHECK (id = 1),
                resets_at       INTEGER NOT NULL,
                start_at        INTEGER NOT NULL,
                used_percentage REAL    NOT NULL,
                updated_at      INTEGER NOT NULL
            );",
        )
        .unwrap();
        (dir, conn)
    }

    #[test]
    fn current_returns_none_when_table_empty() {
        let (_dir, conn) = schema_db();
        let w = Window::current(&conn).unwrap();
        assert!(w.is_none());
    }

    #[test]
    fn current_reads_singleton_row() {
        let (_dir, conn) = schema_db();
        conn.execute(
            "INSERT INTO five_hour_window (id, resets_at, start_at, used_percentage, updated_at) \
             VALUES (1, ?, ?, ?, ?)",
            rusqlite::params![
                1_778_377_597_i64,
                1_778_359_597_i64,
                77.3_f64,
                1_778_365_597_i64
            ],
        )
        .unwrap();
        let w = Window::current(&conn).unwrap().unwrap();
        assert_eq!(w.start_at, 1_778_359_597);
        assert_eq!(w.resets_at, 1_778_377_597);
        assert!((w.used_percentage - 77.3).abs() < 1e-9);
        assert_eq!(w.updated_at, 1_778_365_597);
    }

    #[test]
    fn is_active_when_now_before_reset() {
        let w = Window {
            start_at: 1_000,
            resets_at: 19_000,
            used_percentage: 50.0,
            updated_at: 5_000,
        };
        assert!(w.is_active(10_000));
        assert!(!w.is_active(19_000)); // exclusive
        assert!(!w.is_active(20_000));
    }

    #[test]
    fn seconds_until_reset_arithmetic() {
        let w = Window {
            start_at: 1_000,
            resets_at: 19_000,
            used_percentage: 0.0,
            updated_at: 0,
        };
        assert_eq!(w.seconds_until_reset(10_000), 9_000);
        assert_eq!(w.seconds_until_reset(19_500), -500);
    }
}
