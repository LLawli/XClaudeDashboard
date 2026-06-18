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

    /// Output totals binned into `cols` equal-width time slices spanning the
    /// whole `[start_secs, end_secs)` window, oldest first. Each bin sums all
    /// output that falls in its sub-range, so the series always represents the
    /// *entire* elapsed window (not just the last few minutes) regardless of how
    /// long the window is — for a 7-day window each bin groups roughly an hour.
    /// Returns an empty vec when `cols == 0` or the range is non-positive.
    pub fn binned_series(&self, start_secs: i64, end_secs: i64, cols: u32) -> Vec<u64> {
        if cols == 0 || end_secs <= start_secs {
            return Vec::new();
        }
        let span = (end_secs - start_secs) as f64;
        let n = cols as usize;
        let mut bins = vec![0u64; n];
        for (&bucket, &qty) in &self.buckets {
            let ts = bucket * 60; // minute bucket → its start second
            if ts < start_secs || ts >= end_secs {
                continue;
            }
            let frac = (ts - start_secs) as f64 / span;
            let idx = ((frac * cols as f64) as usize).min(n - 1);
            bins[idx] += qty;
        }
        bins
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
    fn binned_series_distributes_across_whole_window_ordered() {
        let mut r = RateState::new();
        r.replace_from_samples([(at_min(100), 500), (at_min(98), 300)]);
        // window [min 96, min 101) = 300s, 5 bins of 60s each → one bin per minute
        // 96,97,98,99,100 → bin0=0, bin1=0, bin2=300, bin3=0, bin4=500
        assert_eq!(
            r.binned_series(at_min(96), at_min(101), 5),
            vec![0, 0, 300, 0, 500]
        );
    }

    #[test]
    fn binned_series_groups_multiple_minutes_per_bin() {
        let mut r = RateState::new();
        // 4 minutes of data, binned into 2 columns → 2 minutes per bin
        r.replace_from_samples([
            (at_min(0), 100),
            (at_min(1), 200),
            (at_min(2), 40),
            (at_min(3), 60),
        ]);
        // window [min 0, min 4) = 240s, 2 bins of 120s: bin0=min0+1, bin1=min2+3
        assert_eq!(r.binned_series(at_min(0), at_min(4), 2), vec![300, 100]);
    }

    #[test]
    fn binned_series_excludes_samples_outside_range() {
        let mut r = RateState::new();
        r.replace_from_samples([(at_min(50), 1000), (at_min(100), 500), (at_min(200), 999)]);
        // only the min-100 bucket falls inside [min 96, min 101)
        assert_eq!(r.binned_series(at_min(96), at_min(101), 5), vec![0, 0, 0, 0, 500]);
    }

    #[test]
    fn binned_series_empty_range_is_empty() {
        let mut r = RateState::new();
        r.replace_from_samples([(at_min(100), 500)]);
        assert!(r.binned_series(at_min(100), at_min(100), 5).is_empty());
        assert!(r.binned_series(at_min(101), at_min(100), 5).is_empty());
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
    fn binned_series_zero_cols_is_empty() {
        let r = RateState::new();
        assert!(r.binned_series(at_min(0), at_min(100), 0).is_empty());
    }

    #[test]
    fn binned_series_length_matches_cols() {
        let r = RateState::new();
        assert_eq!(r.binned_series(at_min(0), at_min(100), 12).len(), 12);
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
