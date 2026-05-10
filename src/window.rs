use color_eyre::Result;
use rusqlite::{Connection, OptionalExtension};

/// Identifies which singleton table to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    FiveHour,
    SevenDay,
}

impl WindowKind {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::FiveHour => "5h",
            Self::SevenDay => "7d",
        }
    }
}

/// Snapshot of the active session window. Read from `five_hour_window` or
/// `seven_day_window` (both written by `xclaude-usage.js` in XClaudeUsage).
/// All timestamps are **epoch seconds**, both tables are singletons (`id = 1`).
/// The 7d table column is `starts_at` (with an `s`); we alias it to `start_at`
/// in the SELECT so the struct stays unified.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub start_at: i64,
    pub resets_at: i64,
    pub used_percentage: f64,
    pub updated_at: i64,
}

impl Window {
    /// Reads the singleton row for the given kind. Returns `Ok(None)` if the
    /// row hasn't been written yet (XClaudeUsage hasn't fired the first hook
    /// of the new schema). Returns `Err` only on real SQLite errors.
    pub fn current(conn: &Connection, kind: WindowKind) -> Result<Option<Self>> {
        let sql = match kind {
            WindowKind::FiveHour => {
                "SELECT start_at, resets_at, used_percentage, updated_at \
                 FROM five_hour_window WHERE id = 1"
            }
            WindowKind::SevenDay => {
                "SELECT starts_at AS start_at, resets_at, used_percentage, updated_at \
                 FROM seven_day_window WHERE id = 1"
            }
        };
        let row = conn
            .query_row(sql, [], |r| {
                Ok(Self {
                    start_at: r.get(0)?,
                    resets_at: r.get(1)?,
                    used_percentage: r.get(2)?,
                    updated_at: r.get(3)?,
                })
            })
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
            );
            CREATE TABLE seven_day_window (
                id              INTEGER PRIMARY KEY CHECK (id = 1),
                resets_at       INTEGER NOT NULL,
                starts_at       INTEGER NOT NULL,
                used_percentage REAL    NOT NULL,
                updated_at      INTEGER NOT NULL
            );",
        )
        .unwrap();
        (dir, conn)
    }

    #[test]
    fn current_five_hour_returns_none_when_table_empty() {
        let (_dir, conn) = schema_db();
        let w = Window::current(&conn, WindowKind::FiveHour).unwrap();
        assert!(w.is_none());
    }

    #[test]
    fn current_five_hour_reads_singleton_row() {
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
        let w = Window::current(&conn, WindowKind::FiveHour)
            .unwrap()
            .unwrap();
        assert_eq!(w.start_at, 1_778_359_597);
        assert_eq!(w.resets_at, 1_778_377_597);
        assert!((w.used_percentage - 77.3).abs() < 1e-9);
        assert_eq!(w.updated_at, 1_778_365_597);
    }

    #[test]
    fn current_seven_day_returns_none_when_table_empty() {
        let (_dir, conn) = schema_db();
        let w = Window::current(&conn, WindowKind::SevenDay).unwrap();
        assert!(w.is_none());
    }

    #[test]
    fn current_seven_day_aliases_starts_at_to_start_at() {
        let (_dir, conn) = schema_db();
        conn.execute(
            "INSERT INTO seven_day_window (id, resets_at, starts_at, used_percentage, updated_at) \
             VALUES (1, ?, ?, ?, ?)",
            rusqlite::params![
                1_778_461_200_i64,
                1_777_856_400_i64,
                66.0_f64,
                1_778_374_857_i64
            ],
        )
        .unwrap();
        let w = Window::current(&conn, WindowKind::SevenDay)
            .unwrap()
            .unwrap();
        assert_eq!(w.start_at, 1_777_856_400); // sourced from starts_at
        assert_eq!(w.resets_at, 1_778_461_200);
        assert!((w.used_percentage - 66.0).abs() < 1e-9);
        assert_eq!(w.updated_at, 1_778_374_857);
        // Sanity: exact 7d window length
        assert_eq!(w.resets_at - w.start_at, 7 * 24 * 3600);
    }

    #[test]
    fn current_kinds_are_independent() {
        let (_dir, conn) = schema_db();
        // Only the 7d table has a row; querying 5h must return None.
        conn.execute(
            "INSERT INTO seven_day_window (id, resets_at, starts_at, used_percentage, updated_at) \
             VALUES (1, ?, ?, ?, ?)",
            rusqlite::params![100_i64, 50_i64, 10.0_f64, 80_i64],
        )
        .unwrap();
        assert!(
            Window::current(&conn, WindowKind::FiveHour)
                .unwrap()
                .is_none()
        );
        assert!(
            Window::current(&conn, WindowKind::SevenDay)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn label_strings() {
        assert_eq!(WindowKind::FiveHour.label(), "5h");
        assert_eq!(WindowKind::SevenDay.label(), "7d");
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
