use rusqlite::Connection;

#[test]
fn open_existing_db_and_read_data_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("test.db");

    {
        let seed = Connection::open(&path).expect("seed open");
        seed.execute_batch("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1);")
            .expect("seed schema");
    }

    let conn = xclaude::db::open(&path).expect("open db");
    let v = xclaude::db::data_version(&conn).expect("data_version");
    assert!(v >= 1, "data_version should be >= 1, got {v}");
}

#[test]
fn open_missing_db_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("does-not-exist.db");
    let err = xclaude::db::open(&path).expect_err("missing db must error");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("not found"),
        "error should mention 'not found', got: {msg}"
    );
}
