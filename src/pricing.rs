use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::{WrapErr, eyre};
use serde::{Deserialize, Serialize};

const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_creation: f64,
    pub cache_read: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PricingCache {
    /// epoch seconds
    pub fetched_at: i64,
    pub source_url: String,
    pub models: HashMap<String, ModelPrice>,
}

#[derive(Debug, Deserialize)]
struct LiteLlmEntry {
    #[serde(default)]
    input_cost_per_token: Option<f64>,
    #[serde(default)]
    output_cost_per_token: Option<f64>,
    #[serde(default)]
    cache_creation_input_token_cost: Option<f64>,
    #[serde(default)]
    cache_read_input_token_cost: Option<f64>,
}

impl PricingCache {
    /// Load a previously-saved cache from disk. `None` if missing or unparseable.
    pub fn load_from_disk(path: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Persist to disk. Creates parent directories if needed.
    pub fn save_to_disk(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err_with(|| format!("creating parent directory {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)
            .wrap_err_with(|| format!("writing pricing cache to {}", path.display()))?;
        Ok(())
    }

    pub fn is_stale(&self, now_secs: i64, ttl_hours: u32) -> bool {
        let age = now_secs - self.fetched_at;
        age > (ttl_hours as i64) * 3600
    }

    /// Exact match first; otherwise the longest prefix that the model name starts with.
    /// Example: `claude-haiku-4-5-20251001` falls back to `claude-haiku-4-5` if present.
    pub fn lookup(&self, model: &str) -> Option<ModelPrice> {
        if let Some(p) = self.models.get(model) {
            return Some(*p);
        }
        let mut best: Option<(&str, ModelPrice)> = None;
        for (k, v) in &self.models {
            if model.starts_with(k.as_str()) && best.is_none_or(|(prev, _)| k.len() > prev.len()) {
                best = Some((k.as_str(), *v));
            }
        }
        best.map(|(_, p)| p)
    }

    pub fn has(&self, model: &str) -> bool {
        self.lookup(model).is_some()
    }
}

/// Parse the raw LiteLLM JSON, retain only `claude-*` entries, and bundle
/// into a `PricingCache`. The `now_secs` parameter records the fetch instant.
pub fn parse_litellm(raw: &str, now_secs: i64) -> Result<PricingCache> {
    let map: HashMap<String, serde_json::Value> = serde_json::from_str(raw)?;
    let mut models = HashMap::new();
    for (k, v) in map {
        if !k.starts_with("claude-") {
            continue;
        }
        let Ok(entry) = serde_json::from_value::<LiteLlmEntry>(v) else {
            continue;
        };
        let price = ModelPrice {
            input: entry.input_cost_per_token.unwrap_or(0.0),
            output: entry.output_cost_per_token.unwrap_or(0.0),
            cache_creation: entry.cache_creation_input_token_cost.unwrap_or(0.0),
            cache_read: entry.cache_read_input_token_cost.unwrap_or(0.0),
        };
        models.insert(k, price);
    }
    if models.is_empty() {
        return Err(eyre!("no `claude-*` entries in LiteLLM payload"));
    }
    Ok(PricingCache {
        fetched_at: now_secs,
        source_url: LITELLM_URL.into(),
        models,
    })
}

/// Async fetch from the LiteLLM JSON URL.
pub async fn fetch_from_litellm() -> Result<PricingCache> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("xclaude/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let raw = client
        .get(LITELLM_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let now_secs = time::OffsetDateTime::now_utc().unix_timestamp();
    parse_litellm(&raw, now_secs)
}

pub fn cost_for(
    price: ModelPrice,
    input: u64,
    output: u64,
    cache_creation: u64,
    cache_read: u64,
) -> f64 {
    (input as f64) * price.input
        + (output as f64) * price.output
        + (cache_creation as f64) * price.cache_creation
        + (cache_read as f64) * price.cache_read
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/litellm-sample.json");

    #[test]
    fn parse_keeps_only_claude_models() {
        let cache = parse_litellm(FIXTURE, 1_000_000_000).unwrap();
        assert!(cache.models.contains_key("claude-opus-4-7"));
        assert!(cache.models.contains_key("claude-sonnet-4-6"));
        assert!(cache.models.contains_key("claude-haiku-4-5"));
        assert!(!cache.models.contains_key("gpt-4-turbo"));
        assert!(!cache.models.contains_key("sample_spec"));
    }

    #[test]
    fn parse_fills_missing_cache_fields_with_zero() {
        let cache = parse_litellm(FIXTURE, 0).unwrap();
        let haiku = cache.models["claude-haiku-4-5"];
        assert_eq!(haiku.cache_creation, 0.0);
        assert_eq!(haiku.cache_read, 0.0);
        assert!(haiku.input > 0.0);
        assert!(haiku.output > 0.0);
    }

    #[test]
    fn parse_records_fetched_at() {
        let cache = parse_litellm(FIXTURE, 1_778_000_000).unwrap();
        assert_eq!(cache.fetched_at, 1_778_000_000);
        assert_eq!(cache.source_url, LITELLM_URL);
    }

    #[test]
    fn lookup_exact_match_returns_some() {
        let cache = parse_litellm(FIXTURE, 0).unwrap();
        let p = cache.lookup("claude-opus-4-7").unwrap();
        assert!((p.input - 0.000015).abs() < 1e-12);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let cache = parse_litellm(FIXTURE, 0).unwrap();
        assert!(cache.lookup("nope").is_none());
    }

    #[test]
    fn lookup_falls_back_to_longest_prefix() {
        let cache = parse_litellm(FIXTURE, 0).unwrap();
        let p = cache.lookup("claude-haiku-4-5-20251001").unwrap();
        let exact = cache.lookup("claude-haiku-4-5").unwrap();
        assert_eq!(p.input, exact.input);
        assert_eq!(p.output, exact.output);
    }

    #[test]
    fn is_stale_true_when_older_than_ttl() {
        let cache = PricingCache {
            fetched_at: 0,
            source_url: String::new(),
            models: HashMap::new(),
        };
        assert!(cache.is_stale(25 * 3600, 24));
    }

    #[test]
    fn is_stale_false_when_within_ttl() {
        let cache = PricingCache {
            fetched_at: 1_000,
            source_url: String::new(),
            models: HashMap::new(),
        };
        assert!(!cache.is_stale(1_000 + 23 * 3600, 24));
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("prices.json");
        let cache = parse_litellm(FIXTURE, 1_000).unwrap();
        cache.save_to_disk(&path).unwrap();
        let reloaded = PricingCache::load_from_disk(&path).unwrap();
        assert_eq!(reloaded.fetched_at, 1_000);
        assert_eq!(reloaded.models.len(), cache.models.len());
        assert!(reloaded.models.contains_key("claude-opus-4-7"));
    }

    #[test]
    fn load_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        assert!(PricingCache::load_from_disk(&path).is_none());
    }

    #[test]
    fn cost_for_sums_components() {
        let p = ModelPrice {
            input: 0.000_010,
            output: 0.000_050,
            cache_creation: 0.000_020,
            cache_read: 0.000_001,
        };
        // 1000 * 0.00001 = 0.01
        // 2000 * 0.00005 = 0.10
        //  500 * 0.00002 = 0.01
        // 5000 * 0.000001 = 0.005
        // total = 0.125
        let c = cost_for(p, 1_000, 2_000, 500, 5_000);
        assert!((c - 0.125).abs() < 1e-9, "got {c}");
    }

    #[test]
    #[ignore = "requires network; opt in via XCLAUDE_LIVE_TEST=1"]
    fn live_fetch_litellm() {
        if std::env::var("XCLAUDE_LIVE_TEST").is_err() {
            return;
        }
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let cache = rt.block_on(fetch_from_litellm()).unwrap();
        assert!(
            cache.models.len() >= 3,
            "expected at least 3 claude models, got {}",
            cache.models.len()
        );
    }
}
