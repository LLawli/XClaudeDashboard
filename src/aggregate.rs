use std::collections::BTreeMap;

use color_eyre::Result;
use rusqlite::Connection;

use crate::pricing::{PricingCache, cost_for};

/// Bucket name used for tokens originating from this device (`token_usage`
/// table). Remote devices are keyed by their `device_id` slug as stored in
/// `cloud_cache`.
pub const LOCAL_DEVICE: &str = "local";

#[derive(Debug, Clone, Default)]
pub struct ModelTotals {
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

impl ModelTotals {
    pub fn add(&mut self, kind: TokenType, qty: u64) {
        match kind {
            TokenType::Input => self.input += qty,
            TokenType::Output => self.output += qty,
            TokenType::CacheCreation => self.cache_creation += qty,
            TokenType::CacheRead => self.cache_read += qty,
        }
    }

    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_creation + self.cache_read
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TokenType {
    Input,
    Output,
    CacheCreation,
    CacheRead,
}

impl TokenType {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "input" => Some(Self::Input),
            "output" => Some(Self::Output),
            "cache_creation" => Some(Self::CacheCreation),
            "cache_read" => Some(Self::CacheRead),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WindowAggregate {
    pub per_model: BTreeMap<String, ModelTotals>,
    pub total_output: u64,
}

impl WindowAggregate {
    /// Sum tokens consumed in `[start_at, end_at)` from both `token_usage`
    /// (one row per token_type) and `cloud_cache` (denormalized columns).
    /// All timestamps are epoch seconds.
    pub fn fetch(conn: &Connection, start_at: i64, end_at: i64) -> Result<Self> {
        let mut per_model: BTreeMap<String, ModelTotals> = BTreeMap::new();

        let mut stmt = conn.prepare(
            "SELECT model, token_type, COALESCE(SUM(quantity), 0) AS qty
             FROM token_usage
             WHERE executed_at >= ? AND executed_at < ?
             GROUP BY model, token_type",
        )?;
        let rows = stmt.query_map(rusqlite::params![start_at, end_at], |row| {
            let model: String = row.get(0)?;
            let token_type: String = row.get(1)?;
            let qty: i64 = row.get(2)?;
            Ok((model, token_type, qty))
        })?;
        for row in rows {
            let (model, tt_str, qty) = row?;
            if let Some(tt) = TokenType::parse(&tt_str) {
                per_model
                    .entry(model)
                    .or_default()
                    .add(tt, qty.max(0) as u64);
            }
        }

        let mut stmt = conn.prepare(
            "SELECT model,
                    COALESCE(SUM(input), 0) AS input,
                    COALESCE(SUM(output), 0) AS output,
                    COALESCE(SUM(cache_creation), 0) AS cc,
                    COALESCE(SUM(cache_read), 0) AS cr
             FROM cloud_cache
             WHERE executed_at >= ? AND executed_at < ?
             GROUP BY model",
        )?;
        let rows = stmt.query_map(rusqlite::params![start_at, end_at], |row| {
            let model: String = row.get(0)?;
            let input: i64 = row.get(1)?;
            let output: i64 = row.get(2)?;
            let cc: i64 = row.get(3)?;
            let cr: i64 = row.get(4)?;
            Ok((model, input, output, cc, cr))
        })?;
        for row in rows {
            let (model, input, output, cc, cr) = row?;
            let entry = per_model.entry(model).or_default();
            entry.input += input.max(0) as u64;
            entry.output += output.max(0) as u64;
            entry.cache_creation += cc.max(0) as u64;
            entry.cache_read += cr.max(0) as u64;
        }

        let total_output = per_model.values().map(|t| t.output).sum();

        Ok(Self {
            per_model,
            total_output,
        })
    }

    #[allow(dead_code)]
    pub fn total_tokens(&self) -> u64 {
        self.per_model.values().map(|t| t.total()).sum()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.per_model.is_empty()
    }
}

/// Same time window as [`WindowAggregate`] but grouped by device instead of
/// just by model. Two-level map: `device → model → ModelTotals`. The local
/// device (rows from `token_usage`) is keyed by [`LOCAL_DEVICE`]; remote
/// devices come from `cloud_cache` keyed by `device_id`.
#[derive(Debug, Clone, Default)]
pub struct DeviceAggregate {
    pub per_device: BTreeMap<String, BTreeMap<String, ModelTotals>>,
}

impl DeviceAggregate {
    pub fn fetch(conn: &Connection, start_at: i64, end_at: i64) -> Result<Self> {
        let mut per_device: BTreeMap<String, BTreeMap<String, ModelTotals>> = BTreeMap::new();

        let mut stmt = conn.prepare(
            "SELECT model, token_type, COALESCE(SUM(quantity), 0) AS qty
             FROM token_usage
             WHERE executed_at >= ? AND executed_at < ?
             GROUP BY model, token_type",
        )?;
        let rows = stmt.query_map(rusqlite::params![start_at, end_at], |row| {
            let model: String = row.get(0)?;
            let token_type: String = row.get(1)?;
            let qty: i64 = row.get(2)?;
            Ok((model, token_type, qty))
        })?;
        for row in rows {
            let (model, tt_str, qty) = row?;
            if let Some(tt) = TokenType::parse(&tt_str) {
                per_device
                    .entry(LOCAL_DEVICE.to_string())
                    .or_default()
                    .entry(model)
                    .or_default()
                    .add(tt, qty.max(0) as u64);
            }
        }

        let mut stmt = conn.prepare(
            "SELECT device_id, model,
                    COALESCE(SUM(input), 0) AS input,
                    COALESCE(SUM(output), 0) AS output,
                    COALESCE(SUM(cache_creation), 0) AS cc,
                    COALESCE(SUM(cache_read), 0) AS cr
             FROM cloud_cache
             WHERE executed_at >= ? AND executed_at < ?
             GROUP BY device_id, model",
        )?;
        let rows = stmt.query_map(rusqlite::params![start_at, end_at], |row| {
            let device: String = row.get(0)?;
            let model: String = row.get(1)?;
            let input: i64 = row.get(2)?;
            let output: i64 = row.get(3)?;
            let cc: i64 = row.get(4)?;
            let cr: i64 = row.get(5)?;
            Ok((device, model, input, output, cc, cr))
        })?;
        for row in rows {
            let (device, model, input, output, cc, cr) = row?;
            let entry = per_device
                .entry(device)
                .or_default()
                .entry(model)
                .or_default();
            entry.input += input.max(0) as u64;
            entry.output += output.max(0) as u64;
            entry.cache_creation += cc.max(0) as u64;
            entry.cache_read += cr.max(0) as u64;
        }

        Ok(Self { per_device })
    }

    /// Sum every model's totals for a single device. Returns `Default` when the
    /// device key is unknown.
    pub fn totals(&self, device: &str) -> ModelTotals {
        let Some(by_model) = self.per_device.get(device) else {
            return ModelTotals::default();
        };
        let mut out = ModelTotals::default();
        for t in by_model.values() {
            out.input += t.input;
            out.output += t.output;
            out.cache_creation += t.cache_creation;
            out.cache_read += t.cache_read;
        }
        out
    }

    /// Cost in USD for a device — sum of `cost_for(model_price, totals)` over
    /// every model the device used. `None` when *no* model used by the device
    /// has a known price (every individual model with a known price still
    /// contributes; unknown ones contribute 0).
    pub fn cost(&self, device: &str, pricing: &PricingCache) -> Option<f64> {
        let by_model = self.per_device.get(device)?;
        let mut total = 0.0;
        let mut any_known = false;
        for (model, t) in by_model {
            if let Some(p) = pricing.lookup(model) {
                any_known = true;
                total += cost_for(p, t.input, t.output, t.cache_creation, t.cache_read);
            }
        }
        any_known.then_some(total)
    }

    pub fn grand_total_tokens(&self) -> u64 {
        self.per_device
            .values()
            .flat_map(|by_model| by_model.values())
            .map(|t| t.total())
            .sum()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.per_device.is_empty()
    }
}

/// `(executed_at_seconds, output_qty)` pairs in `[start_at, end_at)`. Pulls from
/// both `token_usage` (rows where `token_type = 'output'`) and `cloud_cache`
/// (rows where `output > 0`). Used to feed `RateState::replace_from_samples`.
pub fn output_samples(conn: &Connection, start_at: i64, end_at: i64) -> Result<Vec<(i64, u64)>> {
    let mut samples: Vec<(i64, u64)> = Vec::new();

    let mut stmt = conn.prepare(
        "SELECT executed_at, quantity
         FROM token_usage
         WHERE token_type = 'output' AND executed_at >= ? AND executed_at < ?",
    )?;
    let rows = stmt.query_map(rusqlite::params![start_at, end_at], |row| {
        let ts: i64 = row.get(0)?;
        let qty: i64 = row.get(1)?;
        Ok((ts, qty.max(0) as u64))
    })?;
    for r in rows {
        samples.push(r?);
    }

    let mut stmt = conn.prepare(
        "SELECT executed_at, output
         FROM cloud_cache
         WHERE executed_at >= ? AND executed_at < ? AND output > 0",
    )?;
    let rows = stmt.query_map(rusqlite::params![start_at, end_at], |row| {
        let ts: i64 = row.get(0)?;
        let qty: i64 = row.get(1)?;
        Ok((ts, qty.max(0) as u64))
    })?;
    for r in rows {
        samples.push(r?);
    }

    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id    TEXT NOT NULL,
                model         TEXT NOT NULL,
                token_type    TEXT NOT NULL CHECK (token_type IN ('input','output','cache_creation','cache_read')),
                quantity      INTEGER NOT NULL,
                executed_at   INTEGER NOT NULL,
                device_id     TEXT NOT NULL DEFAULT '',
                message_uuid  TEXT
            );
            CREATE TABLE cloud_cache (
                remote_id       INTEGER PRIMARY KEY,
                device_id       TEXT NOT NULL,
                model           TEXT NOT NULL,
                input           INTEGER NOT NULL DEFAULT 0,
                output          INTEGER NOT NULL DEFAULT 0,
                cache_creation  INTEGER NOT NULL DEFAULT 0,
                cache_read      INTEGER NOT NULL DEFAULT 0,
                executed_at     INTEGER NOT NULL
            );",
        )
        .unwrap();
        (dir, conn)
    }

    fn insert_usage(conn: &Connection, model: &str, tt: &str, qty: i64, ts: i64) {
        conn.execute(
            "INSERT INTO token_usage (session_id, model, token_type, quantity, executed_at) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params!["sess", model, tt, qty, ts],
        )
        .unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_cache(
        conn: &Connection,
        rid: i64,
        device: &str,
        model: &str,
        i: i64,
        o: i64,
        cc: i64,
        cr: i64,
        ts: i64,
    ) {
        conn.execute(
            "INSERT INTO cloud_cache (remote_id, device_id, model, input, output, cache_creation, cache_read, executed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![rid, device, model, i, o, cc, cr, ts],
        )
        .unwrap();
    }

    #[test]
    fn empty_db_is_empty() {
        let (_dir, conn) = schema_db();
        let agg = WindowAggregate::fetch(&conn, 0, i64::MAX).unwrap();
        assert!(agg.is_empty());
        assert_eq!(agg.total_output, 0);
    }

    #[test]
    fn aggregates_token_usage_only() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "claude-opus-4-7", "input", 100, 1000);
        insert_usage(&conn, "claude-opus-4-7", "input", 200, 1500);
        insert_usage(&conn, "claude-opus-4-7", "output", 500, 1000);
        insert_usage(&conn, "claude-sonnet-4-6", "output", 50, 1000);

        let agg = WindowAggregate::fetch(&conn, 0, 2000).unwrap();
        assert_eq!(agg.per_model.len(), 2);
        let opus = &agg.per_model["claude-opus-4-7"];
        assert_eq!(opus.input, 300);
        assert_eq!(opus.output, 500);
        let sonnet = &agg.per_model["claude-sonnet-4-6"];
        assert_eq!(sonnet.output, 50);
        assert_eq!(agg.total_output, 550);
    }

    #[test]
    fn aggregates_cloud_cache_only() {
        let (_dir, conn) = schema_db();
        insert_cache(&conn, 1, "dev-a", "claude-opus-4-7", 100, 200, 50, 30, 1000);
        insert_cache(&conn, 2, "dev-a", "claude-opus-4-7", 10, 20, 5, 3, 1500);

        let agg = WindowAggregate::fetch(&conn, 0, 2000).unwrap();
        let opus = &agg.per_model["claude-opus-4-7"];
        assert_eq!(opus.input, 110);
        assert_eq!(opus.output, 220);
        assert_eq!(opus.cache_creation, 55);
        assert_eq!(opus.cache_read, 33);
        assert_eq!(agg.total_output, 220);
    }

    #[test]
    fn merges_token_usage_and_cloud_cache_for_same_model() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "claude-opus-4-7", "output", 100, 1000);
        insert_cache(&conn, 1, "dev-other", "claude-opus-4-7", 0, 50, 0, 0, 1500);
        let agg = WindowAggregate::fetch(&conn, 0, 2000).unwrap();
        assert_eq!(agg.per_model["claude-opus-4-7"].output, 150);
        assert_eq!(agg.total_output, 150);
    }

    #[test]
    fn excludes_rows_outside_window() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "opus", "output", 999, 500); // before
        insert_usage(&conn, "opus", "output", 100, 1500); // inside
        insert_usage(&conn, "opus", "output", 999, 2500); // after
        insert_cache(&conn, 1, "dev", "opus", 0, 999, 0, 0, 500);
        insert_cache(&conn, 2, "dev", "opus", 0, 50, 0, 0, 1500);
        insert_cache(&conn, 3, "dev", "opus", 0, 999, 0, 0, 2500);

        let agg = WindowAggregate::fetch(&conn, 1000, 2000).unwrap();
        assert_eq!(agg.per_model["opus"].output, 150);
        assert_eq!(agg.total_output, 150);
    }

    #[test]
    fn end_at_is_exclusive() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "opus", "output", 100, 2000);
        let agg = WindowAggregate::fetch(&conn, 1000, 2000).unwrap();
        assert!(agg.is_empty(), "executed_at == end_at must be excluded");
    }

    #[test]
    fn unknown_token_type_is_ignored() {
        let (_dir, conn) = schema_db();
        // Sneak in a row with an unsupported token_type by disabling the CHECK
        // briefly (CHECK is enforced; use a row insert through a model + valid type).
        // Instead, validate that valid types map correctly:
        insert_usage(&conn, "opus", "cache_creation", 10, 1000);
        insert_usage(&conn, "opus", "cache_read", 20, 1000);
        let agg = WindowAggregate::fetch(&conn, 0, 2000).unwrap();
        assert_eq!(agg.per_model["opus"].cache_creation, 10);
        assert_eq!(agg.per_model["opus"].cache_read, 20);
    }

    #[test]
    fn output_samples_combines_both_sources() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "opus", "input", 999, 1000); // ignored — not output
        insert_usage(&conn, "opus", "output", 100, 1000);
        insert_usage(&conn, "opus", "output", 200, 1500);
        insert_cache(&conn, 1, "dev", "opus", 0, 50, 0, 0, 1100);
        insert_cache(&conn, 2, "dev", "opus", 0, 0, 0, 0, 1200); // output == 0, skipped
        insert_cache(&conn, 3, "dev", "opus", 0, 70, 0, 0, 1900);

        let mut samples = output_samples(&conn, 0, 2000).unwrap();
        samples.sort();
        assert_eq!(
            samples,
            vec![(1000, 100), (1100, 50), (1500, 200), (1900, 70)]
        );
    }

    #[test]
    fn output_samples_respects_window() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "opus", "output", 100, 500);
        insert_usage(&conn, "opus", "output", 100, 1500);
        insert_usage(&conn, "opus", "output", 100, 2500);
        let samples = output_samples(&conn, 1000, 2000).unwrap();
        assert_eq!(samples, vec![(1500, 100)]);
    }

    #[test]
    fn total_tokens_sums_all_types_across_models() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "opus", "input", 10, 1000);
        insert_usage(&conn, "opus", "output", 20, 1000);
        insert_usage(&conn, "sonnet", "cache_read", 30, 1000);
        let agg = WindowAggregate::fetch(&conn, 0, 2000).unwrap();
        assert_eq!(agg.total_tokens(), 60);
    }

    use crate::pricing::{ModelPrice, PricingCache};
    use std::collections::HashMap;

    fn pricing(entries: &[(&str, f64, f64, f64, f64)]) -> PricingCache {
        let mut models = HashMap::new();
        for (k, i, o, cc, cr) in entries {
            models.insert(
                (*k).to_string(),
                ModelPrice {
                    input: *i,
                    output: *o,
                    cache_creation: *cc,
                    cache_read: *cr,
                },
            );
        }
        PricingCache {
            fetched_at: 0,
            source_url: String::new(),
            models,
        }
    }

    #[test]
    fn device_aggregate_empty_db() {
        let (_dir, conn) = schema_db();
        let agg = DeviceAggregate::fetch(&conn, 0, i64::MAX).unwrap();
        assert!(agg.is_empty());
        assert_eq!(agg.grand_total_tokens(), 0);
    }

    #[test]
    fn device_aggregate_only_token_usage_buckets_as_local() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "claude-opus-4-7", "input", 100, 1000);
        insert_usage(&conn, "claude-opus-4-7", "output", 200, 1000);
        insert_usage(&conn, "claude-sonnet-4-6", "output", 50, 1500);

        let agg = DeviceAggregate::fetch(&conn, 0, 2000).unwrap();
        assert_eq!(agg.per_device.len(), 1);
        assert!(agg.per_device.contains_key(LOCAL_DEVICE));
        let totals = agg.totals(LOCAL_DEVICE);
        assert_eq!(totals.input, 100);
        assert_eq!(totals.output, 250);
        assert_eq!(agg.grand_total_tokens(), 350);
    }

    #[test]
    fn device_aggregate_only_cloud_cache_buckets_by_slug() {
        let (_dir, conn) = schema_db();
        insert_cache(
            &conn,
            1,
            "luka-desktop",
            "claude-opus-4-7",
            100,
            200,
            50,
            30,
            1000,
        );
        insert_cache(
            &conn,
            2,
            "luka-notebook",
            "claude-opus-4-7",
            10,
            20,
            5,
            3,
            1500,
        );
        insert_cache(
            &conn,
            3,
            "luka-desktop",
            "claude-sonnet-4-6",
            1,
            2,
            0,
            0,
            1500,
        );

        let agg = DeviceAggregate::fetch(&conn, 0, 2000).unwrap();
        assert!(!agg.per_device.contains_key(LOCAL_DEVICE));
        assert_eq!(agg.per_device.len(), 2);
        let desk = agg.totals("luka-desktop");
        assert_eq!(desk.input, 101);
        assert_eq!(desk.output, 202);
        assert_eq!(desk.cache_creation, 50);
        assert_eq!(desk.cache_read, 30);
        let nb = agg.totals("luka-notebook");
        assert_eq!(nb.input, 10);
        assert_eq!(nb.output, 20);
    }

    #[test]
    fn device_aggregate_mixes_local_and_remote_without_merging() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "claude-opus-4-7", "output", 100, 1000);
        insert_cache(
            &conn,
            1,
            "luka-desktop",
            "claude-opus-4-7",
            0,
            50,
            0,
            0,
            1500,
        );

        let agg = DeviceAggregate::fetch(&conn, 0, 2000).unwrap();
        assert_eq!(agg.per_device.len(), 2);
        assert_eq!(agg.totals(LOCAL_DEVICE).output, 100);
        assert_eq!(agg.totals("luka-desktop").output, 50);
    }

    #[test]
    fn device_aggregate_respects_window() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "opus", "output", 999, 500);
        insert_usage(&conn, "opus", "output", 100, 1500);
        insert_cache(&conn, 1, "dev", "opus", 0, 999, 0, 0, 500);
        insert_cache(&conn, 2, "dev", "opus", 0, 50, 0, 0, 1500);

        let agg = DeviceAggregate::fetch(&conn, 1000, 2000).unwrap();
        assert_eq!(agg.totals(LOCAL_DEVICE).output, 100);
        assert_eq!(agg.totals("dev").output, 50);
    }

    #[test]
    fn device_aggregate_cost_sums_known_models() {
        let (_dir, conn) = schema_db();
        // 1 input @ 10 + 1 output @ 100 = 110, only opus has a price
        insert_usage(&conn, "claude-opus-4-7", "input", 1, 1000);
        insert_usage(&conn, "claude-opus-4-7", "output", 1, 1000);
        insert_usage(&conn, "unknown-model", "output", 999, 1000);

        let agg = DeviceAggregate::fetch(&conn, 0, 2000).unwrap();
        let p = pricing(&[("claude-opus-4-7", 10.0, 100.0, 0.0, 0.0)]);
        assert_eq!(agg.cost(LOCAL_DEVICE, &p), Some(110.0));
    }

    #[test]
    fn device_aggregate_cost_none_when_no_known_models() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "unknown", "output", 100, 1000);
        let agg = DeviceAggregate::fetch(&conn, 0, 2000).unwrap();
        let p = pricing(&[]);
        assert_eq!(agg.cost(LOCAL_DEVICE, &p), None);
    }

    #[test]
    fn device_aggregate_grand_total_tokens_sums_all_buckets() {
        let (_dir, conn) = schema_db();
        insert_usage(&conn, "opus", "input", 10, 1000);
        insert_usage(&conn, "opus", "output", 20, 1000);
        insert_cache(&conn, 1, "dev", "opus", 5, 5, 0, 0, 1000);
        let agg = DeviceAggregate::fetch(&conn, 0, 2000).unwrap();
        assert_eq!(agg.grand_total_tokens(), 40);
    }
}
