use std::path::Path;
use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::{WrapErr, eyre};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

const WINDOW_SECONDS: i64 = 5 * 3600;
const PULL_LIMIT: i64 = 500;
const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default)]
pub struct SyncOutcome {
    pub pulled_rows: usize,
    pub pushed_rows: usize,
}

/// Pushes pending deltas and pulls remote rows from Turso, writing them back
/// to the same SQLite that XClaudeUsage uses (tables `cloud_outbox`,
/// `cloud_cache`, `cloud_state`). Mirrors `syncCloud()` in
/// `~/Projeto/XClaudeUsage/xclaude-record.js:240-329`.
pub async fn sync_turso(db_path: &Path, cloud_config: &Value) -> Result<SyncOutcome> {
    let cfg = TursoConfig::from_value(cloud_config)?;

    // Phase 1: snapshot local state in a single quick conn open.
    let snapshot = snapshot_local(db_path, cfg.device_id.as_deref())?;

    // Phase 2: build pipeline requests and send to Turso.
    let now_secs = time::OffsetDateTime::now_utc().unix_timestamp();
    let (requests, pull_idx) = build_pipeline_requests(&snapshot, now_secs);
    let response = libsql_pipeline(&cfg.libsql_url, &cfg.auth_token, requests).await?;
    let pulled = check_results_ok_and_extract_pull(&response, pull_idx)?;

    // Phase 3: apply remote results locally inside one transaction.
    let cutoff = now_secs - WINDOW_SECONDS;
    let mut conn = open_local(db_path)?;
    let outcome = apply_local_changes(&mut conn, &snapshot, &pulled, cutoff)?;

    Ok(outcome)
}

#[derive(Debug)]
struct TursoConfig {
    libsql_url: String,
    auth_token: String,
    device_id: Option<String>,
}

impl TursoConfig {
    fn from_value(v: &Value) -> Result<Self> {
        let libsql = v
            .get("libsql_url")
            .and_then(|x| x.as_str())
            .ok_or_else(|| eyre!("xclaude-cloud.json: libsql_url missing or not a string"))?;
        if !libsql.starts_with("libsql://")
            && !libsql.starts_with("https://")
            && !libsql.starts_with("http://")
        {
            return Err(eyre!(
                "xclaude-cloud.json: libsql_url must start with libsql:// or https://"
            ));
        }
        let token = v
            .get("auth_token")
            .and_then(|x| x.as_str())
            .ok_or_else(|| eyre!("xclaude-cloud.json: auth_token missing or not a string"))?;
        if token.is_empty() {
            return Err(eyre!("xclaude-cloud.json: auth_token is empty"));
        }
        let normalized = libsql
            .strip_prefix("libsql://")
            .map(|rest| format!("https://{rest}"))
            .unwrap_or_else(|| libsql.to_string());
        let normalized = normalized.trim_end_matches('/').to_string();
        let device_id = v
            .get("device_id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);
        Ok(Self {
            libsql_url: normalized,
            auth_token: token.to_string(),
            device_id,
        })
    }
}

#[derive(Debug)]
struct LocalSnapshot {
    device_id: String,
    outbox: Vec<(String, String)>, // (event_id, payload JSON string)
    last_remote_id: i64,
}

fn open_local(db_path: &Path) -> Result<Connection> {
    if !db_path.exists() {
        return Err(eyre!(
            "SQLite database not found at {}. XClaudeUsage must run at least once.",
            db_path.display()
        ));
    }
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    )?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(conn)
}

fn snapshot_local(db_path: &Path, override_device_id: Option<&str>) -> Result<LocalSnapshot> {
    let conn = open_local(db_path)?;
    let device_id = ensure_device_id(&conn, override_device_id)?;
    let outbox = read_outbox(&conn)?;
    let last_remote_id = read_cursor(&conn)?;
    Ok(LocalSnapshot {
        device_id,
        outbox,
        last_remote_id,
    })
}

fn ensure_device_id(conn: &Connection, override_id: Option<&str>) -> Result<String> {
    if let Some(id) = override_id {
        return Ok(id.to_string());
    }
    if let Ok(existing) = conn.query_row::<String, _, _>(
        "SELECT value FROM cloud_state WHERE key = 'device_id'",
        [],
        |row| row.get(0),
    ) {
        return Ok(existing);
    }
    let new_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT OR IGNORE INTO cloud_state (key, value) VALUES ('device_id', ?)",
        [&new_id],
    )?;
    let stored: String = conn.query_row(
        "SELECT value FROM cloud_state WHERE key = 'device_id'",
        [],
        |row| row.get(0),
    )?;
    Ok(stored)
}

fn read_outbox(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt =
        conn.prepare("SELECT event_id, payload FROM cloud_outbox ORDER BY created_at ASC")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn read_cursor(conn: &Connection) -> Result<i64> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM cloud_state WHERE key = 'last_remote_id'",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(value.and_then(|v| v.parse::<i64>().ok()).unwrap_or(0))
}

fn lv_int(n: i64) -> Value {
    json!({ "type": "integer", "value": n.to_string() })
}

fn lv_text(s: &str) -> Value {
    json!({ "type": "text", "value": s })
}

fn build_pipeline_requests(snapshot: &LocalSnapshot, now_secs: i64) -> (Vec<Value>, usize) {
    let cutoff = now_secs - WINDOW_SECONDS;
    let mut requests = Vec::with_capacity(snapshot.outbox.len() + 3);

    for (event_id, payload) in &snapshot.outbox {
        let Ok(p) = serde_json::from_str::<Value>(payload) else {
            continue;
        };
        let device_id = p.get("device_id").and_then(|v| v.as_str()).unwrap_or("");
        let model = p.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let event_type = p.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
        let executed_at = p.get("executed_at").and_then(|v| v.as_i64()).unwrap_or(0);
        let input = p.get("input").and_then(|v| v.as_i64()).unwrap_or(0);
        let output = p.get("output").and_then(|v| v.as_i64()).unwrap_or(0);
        let cc = p
            .get("cache_creation")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cr = p.get("cache_read").and_then(|v| v.as_i64()).unwrap_or(0);
        requests.push(json!({
            "type": "execute",
            "stmt": {
                "sql": "INSERT OR IGNORE INTO token_delta \
                        (device_id, model, event_type, input, output, cache_creation, cache_read, executed_at, event_id) \
                        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                "args": [
                    lv_text(device_id), lv_text(model), lv_text(event_type),
                    lv_int(input), lv_int(output), lv_int(cc), lv_int(cr),
                    lv_int(executed_at), lv_text(event_id),
                ]
            }
        }));
    }

    let pull_idx = requests.len();
    requests.push(json!({
        "type": "execute",
        "stmt": {
            "sql": "SELECT id, device_id, model, input, output, cache_creation, cache_read, executed_at \
                    FROM token_delta WHERE id > ? AND device_id != ? AND executed_at >= ? \
                    ORDER BY id ASC LIMIT ?",
            "args": [
                lv_int(snapshot.last_remote_id),
                lv_text(&snapshot.device_id),
                lv_int(cutoff),
                lv_int(PULL_LIMIT),
            ]
        }
    }));

    requests.push(json!({
        "type": "execute",
        "stmt": {
            "sql": "DELETE FROM token_delta WHERE executed_at < ?",
            "args": [lv_int(cutoff)],
        }
    }));

    requests.push(json!({ "type": "close" }));

    (requests, pull_idx)
}

async fn libsql_pipeline(url: &str, token: &str, requests: Vec<Value>) -> Result<Value> {
    let endpoint = format!("{}/v2/pipeline", url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(NETWORK_TIMEOUT)
        .user_agent(concat!("xclaude/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = client
        .post(&endpoint)
        .bearer_auth(token)
        .json(&json!({ "requests": requests }))
        .send()
        .await
        .wrap_err("libsql pipeline HTTP request failed")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let trunc: String = body.chars().take(200).collect();
        return Err(eyre!("libsql {status}: {trunc}"));
    }
    resp.json::<Value>()
        .await
        .wrap_err("libsql pipeline response decode failed")
}

#[derive(Debug, Clone, PartialEq)]
struct PulledRow {
    id: i64,
    device_id: String,
    model: String,
    input: i64,
    output: i64,
    cache_creation: i64,
    cache_read: i64,
    executed_at: i64,
}

fn check_results_ok_and_extract_pull(response: &Value, pull_idx: usize) -> Result<Vec<PulledRow>> {
    let results = response
        .get("results")
        .and_then(|v| v.as_array())
        .ok_or_else(|| eyre!("libsql response: missing `results` array"))?;
    for (i, r) in results.iter().enumerate() {
        let t = r.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t != "ok" {
            let dump: String = serde_json::to_string(r)
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect();
            return Err(eyre!("libsql stmt {i} failed: {dump}"));
        }
    }
    let pull = results
        .get(pull_idx)
        .ok_or_else(|| eyre!("libsql response: pull result missing"))?;
    let exec_result = pull
        .pointer("/response/result")
        .ok_or_else(|| eyre!("libsql response: pull execResult missing"))?;
    decode_rows(exec_result)
}

fn decode_rows(exec_result: &Value) -> Result<Vec<PulledRow>> {
    let cols = exec_result
        .get("cols")
        .and_then(|v| v.as_array())
        .ok_or_else(|| eyre!("libsql exec_result: missing cols"))?;
    let rows = exec_result
        .get("rows")
        .and_then(|v| v.as_array())
        .ok_or_else(|| eyre!("libsql exec_result: missing rows"))?;
    let col_names: Vec<&str> = cols
        .iter()
        .map(|c| c.get("name").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    let idx = |name: &str| -> Result<usize> {
        col_names
            .iter()
            .position(|c| *c == name)
            .ok_or_else(|| eyre!("libsql exec_result: column `{name}` missing"))
    };
    let i_id = idx("id")?;
    let i_dev = idx("device_id")?;
    let i_model = idx("model")?;
    let i_input = idx("input")?;
    let i_output = idx("output")?;
    let i_cc = idx("cache_creation")?;
    let i_cr = idx("cache_read")?;
    let i_ts = idx("executed_at")?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let cells = r
            .as_array()
            .ok_or_else(|| eyre!("libsql row not an array"))?;
        out.push(PulledRow {
            id: cell_int(&cells[i_id])?,
            device_id: cell_text(&cells[i_dev])?,
            model: cell_text(&cells[i_model])?,
            input: cell_int(&cells[i_input])?,
            output: cell_int(&cells[i_output])?,
            cache_creation: cell_int(&cells[i_cc])?,
            cache_read: cell_int(&cells[i_cr])?,
            executed_at: cell_int(&cells[i_ts])?,
        });
    }
    Ok(out)
}

fn cell_int(c: &Value) -> Result<i64> {
    let t = c.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match t {
        "integer" => c
            .get("value")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| eyre!("libsql integer cell: invalid value")),
        "float" => Ok(c.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64),
        "null" => Ok(0),
        other => Err(eyre!("libsql cell: unexpected type `{other}`")),
    }
}

fn cell_text(c: &Value) -> Result<String> {
    let t = c.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match t {
        "text" => Ok(c
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()),
        "null" => Ok(String::new()),
        other => Err(eyre!("libsql cell: unexpected type `{other}`")),
    }
}

fn apply_local_changes(
    conn: &mut Connection,
    snapshot: &LocalSnapshot,
    pulled: &[PulledRow],
    cutoff: i64,
) -> Result<SyncOutcome> {
    let tx = conn.transaction()?;

    for (event_id, _) in &snapshot.outbox {
        tx.execute("DELETE FROM cloud_outbox WHERE event_id = ?", [event_id])?;
    }

    let mut max_id = snapshot.last_remote_id;
    for r in pulled {
        tx.execute(
            "INSERT OR REPLACE INTO cloud_cache \
             (remote_id, device_id, model, input, output, cache_creation, cache_read, executed_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                r.id,
                r.device_id,
                r.model,
                r.input,
                r.output,
                r.cache_creation,
                r.cache_read,
                r.executed_at,
            ],
        )?;
        if r.id > max_id {
            max_id = r.id;
        }
    }
    if max_id > snapshot.last_remote_id {
        tx.execute(
            "INSERT INTO cloud_state (key, value) VALUES ('last_remote_id', ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [max_id.to_string()],
        )?;
    }

    tx.execute("DELETE FROM cloud_cache WHERE executed_at < ?", [cutoff])?;

    tx.commit()?;

    Ok(SyncOutcome {
        pulled_rows: pulled.len(),
        pushed_rows: snapshot.outbox.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parses_libsql_scheme_to_https() {
        let v = json!({
            "libsql_url": "libsql://example.turso.io",
            "auth_token": "tk",
        });
        let c = TursoConfig::from_value(&v).unwrap();
        assert_eq!(c.libsql_url, "https://example.turso.io");
        assert_eq!(c.auth_token, "tk");
        assert!(c.device_id.is_none());
    }

    #[test]
    fn config_strips_trailing_slash() {
        let v = json!({ "libsql_url": "https://x/", "auth_token": "tk" });
        let c = TursoConfig::from_value(&v).unwrap();
        assert_eq!(c.libsql_url, "https://x");
    }

    #[test]
    fn config_picks_up_explicit_device_id() {
        let v = json!({
            "libsql_url": "https://x",
            "auth_token": "tk",
            "device_id": "my-laptop",
        });
        let c = TursoConfig::from_value(&v).unwrap();
        assert_eq!(c.device_id.as_deref(), Some("my-laptop"));
    }

    #[test]
    fn config_rejects_missing_url() {
        let v = json!({ "auth_token": "tk" });
        assert!(TursoConfig::from_value(&v).is_err());
    }

    #[test]
    fn config_rejects_bad_scheme() {
        let v = json!({ "libsql_url": "ftp://x", "auth_token": "tk" });
        assert!(TursoConfig::from_value(&v).is_err());
    }

    #[test]
    fn config_rejects_empty_token() {
        let v = json!({ "libsql_url": "https://x", "auth_token": "" });
        assert!(TursoConfig::from_value(&v).is_err());
    }

    #[test]
    fn decode_rows_parses_typed_cells() {
        let exec = json!({
            "cols": [
                {"name": "id"},
                {"name": "device_id"},
                {"name": "model"},
                {"name": "input"},
                {"name": "output"},
                {"name": "cache_creation"},
                {"name": "cache_read"},
                {"name": "executed_at"},
            ],
            "rows": [
                [
                    {"type": "integer", "value": "42"},
                    {"type": "text", "value": "dev-a"},
                    {"type": "text", "value": "claude-opus-4-7"},
                    {"type": "integer", "value": "100"},
                    {"type": "integer", "value": "200"},
                    {"type": "null"},
                    {"type": "integer", "value": "50"},
                    {"type": "integer", "value": "1778000000"},
                ],
            ],
        });
        let rows = decode_rows(&exec).unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.id, 42);
        assert_eq!(r.device_id, "dev-a");
        assert_eq!(r.model, "claude-opus-4-7");
        assert_eq!(r.input, 100);
        assert_eq!(r.output, 200);
        assert_eq!(r.cache_creation, 0); // null → 0
        assert_eq!(r.cache_read, 50);
        assert_eq!(r.executed_at, 1_778_000_000);
    }

    #[test]
    fn decode_rows_empty() {
        let exec = json!({ "cols": [{"name": "id"}], "rows": [] });
        // missing some required cols → error
        assert!(decode_rows(&exec).is_err());
    }

    #[test]
    fn check_results_extracts_pull_when_all_ok() {
        let resp = json!({
            "results": [
                { "type": "ok", "response": { "type": "execute", "result": {
                    "cols": [
                        {"name": "id"}, {"name": "device_id"}, {"name": "model"},
                        {"name": "input"}, {"name": "output"},
                        {"name": "cache_creation"}, {"name": "cache_read"},
                        {"name": "executed_at"},
                    ],
                    "rows": []
                }}},
                { "type": "ok", "response": { "type": "execute", "result": {
                    "cols": [], "rows": []
                }}},
                { "type": "ok", "response": { "type": "close" } },
            ]
        });
        let pulled = check_results_ok_and_extract_pull(&resp, 0).unwrap();
        assert!(pulled.is_empty());
    }

    #[test]
    fn check_results_errors_on_failed_stmt() {
        let resp = json!({
            "results": [
                { "type": "error", "error": { "message": "table missing" } },
                { "type": "ok", "response": { "type": "close" } },
            ]
        });
        let err = check_results_ok_and_extract_pull(&resp, 0).unwrap_err();
        assert!(err.to_string().contains("stmt 0 failed"));
    }

    #[test]
    fn build_pipeline_includes_pull_and_cleanup_and_close() {
        let snap = LocalSnapshot {
            device_id: "dev-self".into(),
            outbox: vec![],
            last_remote_id: 100,
        };
        let (req, pull_idx) = build_pipeline_requests(&snap, 1_000_000);
        assert_eq!(pull_idx, 0); // no outbox rows, pull is first
        assert_eq!(req.len(), 3); // pull + cleanup + close
        assert_eq!(req[2].get("type").and_then(|v| v.as_str()), Some("close"));
    }

    #[test]
    fn build_pipeline_skips_malformed_outbox_payload() {
        let snap = LocalSnapshot {
            device_id: "dev".into(),
            outbox: vec![("evt-bad".into(), "not json".into())],
            last_remote_id: 0,
        };
        let (req, pull_idx) = build_pipeline_requests(&snap, 0);
        // bad payload skipped → no INSERT, pull is at index 0
        assert_eq!(pull_idx, 0);
        assert_eq!(req.len(), 3);
    }
}
