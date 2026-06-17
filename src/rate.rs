use std::collections::BTreeMap;

/// Per-minute output-token buckets for sliding-window burn rate + ETA.
/// Timestamps are **epoch seconds** (matching `token_usage.executed_at`).
#[derive(Debug, Default)]
pub struct RateState {
    /// minute_bucket (`executed_at_seconds / 60`) → output tokens summed in that minute
    buckets: BTreeMap<i64, u64>,
}

impl RateState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the internal buckets with a fresh aggregation of
    /// `(executed_at_seconds, output_qty)` samples.
    pub fn replace_from_samples<I>(&mut self, samples: I)
    where
        I: IntoIterator<Item = (i64, u64)>,
    {
        self.buckets.clear();
        for (ts, qty) in samples {
            let bucket = ts.div_euclid(60);
            *self.buckets.entry(bucket).or_insert(0) += qty;
        }
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// Tokens-per-minute over the last `window_min` minutes ending at `now_secs`.
    /// Returns `0.0` if no samples in the window or `window_min == 0`.
    pub fn rate_per_minute(&self, now_secs: i64, window_min: u32) -> f64 {
        if window_min == 0 {
            return 0.0;
        }
        let now_bucket = now_secs.div_euclid(60);
        let lo = now_bucket - window_min as i64;
        let total: u64 = self.buckets.range(lo..=now_bucket).map(|(_, &v)| v).sum();
        total as f64 / window_min as f64
    }

    /// Seconds until `remaining` tokens are exhausted at the current burn rate.
    /// Returns `None` if rate is below `idle_threshold_per_min` (extrapolation
    /// not significant). Returns `Some(0)` if `remaining == 0`.
    pub fn eta_seconds(
        &self,
        remaining: u64,
        now_secs: i64,
        window_min: u32,
        idle_threshold_per_min: f64,
    ) -> Option<u64> {
        if remaining == 0 {
            return Some(0);
        }
        let rate = self.rate_per_minute(now_secs, window_min);
        if rate < idle_threshold_per_min {
            return None;
        }
        let per_sec = rate / 60.0;
        Some((remaining as f64 / per_sec).round() as u64)
    }

    /// Per-minute output totals over the last `span_min` minutes ending at the
    /// minute containing `now_secs`, oldest first, with idle minutes filled as
    /// 0 so the series always has exactly `span_min` entries (a stable width
    /// for a sparkline). Empty when `span_min == 0`.
    pub fn minute_series(&self, now_secs: i64, span_min: u32) -> Vec<u64> {
        if span_min == 0 {
            return Vec::new();
        }
        let now_bucket = now_secs.div_euclid(60);
        let start = now_bucket - (span_min as i64) + 1;
        (start..=now_bucket)
            .map(|b| self.buckets.get(&b).copied().unwrap_or(0))
            .collect()
    }

    /// The highest single-minute output total across all buckets in the window
    /// (0 if there are no samples).
    pub fn peak_per_minute(&self) -> u64 {
        self.buckets.values().copied().max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_min(m: i64) -> i64 {
        m * 60
    }

    #[test]
    fn empty_rate_is_zero() {
        let r = RateState::new();
        assert_eq!(r.rate_per_minute(at_min(100), 15), 0.0);
    }

    #[test]
    fn rate_averages_over_window_minutes() {
        let mut r = RateState::new();
        // 6 minutes of 1000 tokens each, ending at min 100
        let samples = (95..=100).map(|m| (at_min(m), 1000));
        r.replace_from_samples(samples);
        // window 15min: 6 buckets * 1000 / 15 = 400/min
        assert!((r.rate_per_minute(at_min(100), 15) - 400.0).abs() < 1e-6);
    }

    #[test]
    fn samples_outside_window_excluded() {
        let mut r = RateState::new();
        r.replace_from_samples([(at_min(50), 1000), (at_min(100), 1000)]);
        // 15min window ending at min 100: only the 100-bucket counts
        assert!((r.rate_per_minute(at_min(100), 15) - (1000.0 / 15.0)).abs() < 1e-6);
    }

    #[test]
    fn rate_with_zero_window_is_zero() {
        let mut r = RateState::new();
        r.replace_from_samples([(at_min(100), 1000)]);
        assert_eq!(r.rate_per_minute(at_min(100), 0), 0.0);
    }

    #[test]
    fn eta_idle_returns_none() {
        let mut r = RateState::new();
        // 50 tokens in 15min → ~3/min, well below 100/min threshold
        r.replace_from_samples([(at_min(100), 50)]);
        assert!(r.eta_seconds(100_000, at_min(100), 15, 100.0).is_none());
    }

    #[test]
    fn eta_active_returns_seconds() {
        let mut r = RateState::new();
        // 30k tokens in last minute → 30000/15 = 2000/min = 33.33/sec
        // remaining 60_000 → 60_000 / 33.33 = 1800 seconds (30 min)
        r.replace_from_samples([(at_min(100), 30_000)]);
        let eta = r.eta_seconds(60_000, at_min(100), 15, 100.0).unwrap();
        assert_eq!(eta, 1800);
    }

    #[test]
    fn eta_zero_remaining_returns_zero() {
        let mut r = RateState::new();
        r.replace_from_samples([(at_min(100), 30_000)]);
        assert_eq!(r.eta_seconds(0, at_min(100), 15, 100.0), Some(0));
    }

    #[test]
    fn replace_from_samples_resets_state() {
        let mut r = RateState::new();
        r.replace_from_samples([(at_min(100), 1000)]);
        assert!(!r.is_empty());
        r.replace_from_samples(std::iter::empty::<(i64, u64)>());
        assert!(r.is_empty());
    }

    #[test]
    fn minute_series_fills_gaps_with_zero_and_is_ordered() {
        let mut r = RateState::new();
        r.replace_from_samples([(at_min(100), 500), (at_min(98), 300)]);
        // last 5 minutes ending at min 100 → buckets 96,97,98,99,100
        assert_eq!(r.minute_series(at_min(100), 5), vec![0, 0, 300, 0, 500]);
    }

    #[test]
    fn peak_per_minute_returns_max_bucket() {
        let mut r = RateState::new();
        r.replace_from_samples([(at_min(100), 500), (at_min(98), 1_200), (at_min(99), 300)]);
        assert_eq!(r.peak_per_minute(), 1_200);
    }

    #[test]
    fn peak_per_minute_empty_is_zero() {
        assert_eq!(RateState::new().peak_per_minute(), 0);
    }

    #[test]
    fn minute_series_zero_span_is_empty() {
        let r = RateState::new();
        assert!(r.minute_series(at_min(100), 0).is_empty());
    }

    #[test]
    fn minute_series_length_matches_span() {
        let r = RateState::new();
        assert_eq!(r.minute_series(at_min(100), 12).len(), 12);
    }

    #[test]
    fn samples_in_same_minute_aggregate() {
        let mut r = RateState::new();
        r.replace_from_samples([
            (at_min(100), 100),
            (at_min(100) + 30, 200),
            (at_min(100) + 59, 300),
        ]);
        // all 3 samples land in bucket 100, totalling 600
        // 600 / 15 = 40/min
        assert!((r.rate_per_minute(at_min(100), 15) - 40.0).abs() < 1e-6);
    }
}
