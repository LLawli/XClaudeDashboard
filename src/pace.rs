//! Daily pace for long windows (the 7d view).
//!
//! The weekly window is split into 24 h slices aligned to its start (so the
//! slice boundary is the reset hour). Each slice gets its share of the
//! reported `used_percentage`, in proportion to the output tokens burned in
//! it, which makes the slices sum exactly to what Claude reports. From that we
//! derive the average spend per day so far and the even daily budget that
//! makes the remaining quota last until the reset.

pub const DAY_SECS: i64 = 86_400;

/// Floor for the elapsed time when averaging, so the first minutes of a window
/// don't extrapolate a tiny spend into an absurd per-day figure.
const MIN_ELAPSED_SECS: i64 = 3_600;

#[derive(Debug, Clone, PartialEq)]
pub struct DailyPace {
    /// Percent of the window's quota spent in each slice, oldest first.
    pub days: Vec<f64>,
    /// Slice containing `now`; `None` before the start or after the reset.
    pub today: Option<usize>,
    /// Percent spent per day so far (used / elapsed days).
    pub avg_per_day: f64,
    /// Remaining percent spread evenly over the time left; `None` once closed.
    pub budget_per_day: Option<f64>,
    /// Today's share of the remaining quota, fixed at the slice start so it
    /// doesn't shrink while you spend it; `None` once closed.
    pub today_budget: Option<f64>,
}

impl DailyPace {
    pub fn today_spent(&self) -> Option<f64> {
        self.today.map(|i| self.days[i])
    }
}

/// Number of 24 h slices covering `[start_at, resets_at)` (7 for the 7d window).
pub fn slice_count(start_at: i64, resets_at: i64) -> usize {
    let span = (resets_at - start_at).max(0);
    ((span + DAY_SECS - 1) / DAY_SECS) as usize
}

/// Builds the pace from the window bounds, the reported usage and the output
/// tokens per slice (`day_output[k]` covers `start_at + k * DAY_SECS`, one day).
pub fn daily_pace(
    start_at: i64,
    resets_at: i64,
    now: i64,
    used_pct: f64,
    day_output: &[u64],
) -> DailyPace {
    // `used_percentage` is an external column: NaN/inf/negative read as 0.
    let used = if used_pct.is_finite() {
        used_pct.max(0.0)
    } else {
        0.0
    };
    let total: u64 = day_output.iter().sum();
    let days: Vec<f64> = day_output
        .iter()
        .map(|&o| {
            if total == 0 {
                0.0
            } else {
                o as f64 / total as f64 * used
            }
        })
        .collect();

    let elapsed = (now.min(resets_at) - start_at).max(MIN_ELAPSED_SECS);
    let avg_per_day = used / (elapsed as f64 / DAY_SECS as f64);

    let open = now >= start_at && now < resets_at && !days.is_empty();
    if !open {
        return DailyPace {
            days,
            today: None,
            avg_per_day,
            budget_per_day: None,
            today_budget: None,
        };
    }

    let remaining = (100.0 - used).max(0.0);
    let days_left = (resets_at - now) as f64 / DAY_SECS as f64;
    let today = (((now - start_at) / DAY_SECS) as usize).min(days.len() - 1);
    let slice_start = start_at + today as i64 * DAY_SECS;
    let slices_left = (resets_at - slice_start) as f64 / DAY_SECS as f64;
    let before_today = used - days[today];

    DailyPace {
        avg_per_day,
        budget_per_day: Some(remaining / days_left),
        today_budget: Some((100.0 - before_today).max(0.0) / slices_left),
        today: Some(today),
        days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: i64 = 1_000_000;
    const WEEK: i64 = 7 * DAY_SECS;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn slice_count_covers_partial_days() {
        assert_eq!(slice_count(START, START + WEEK), 7);
        assert_eq!(slice_count(START, START + 5 * 3600), 1);
        assert_eq!(slice_count(START, START), 0);
        assert_eq!(slice_count(START, START - 10), 0);
    }

    #[test]
    fn days_split_used_pct_by_output_share() {
        let p = daily_pace(
            START,
            START + WEEK,
            START + 2 * DAY_SECS + 3600,
            60.0,
            &[100, 200, 300, 0, 0, 0, 0],
        );
        assert!(close(p.days[0], 10.0));
        assert!(close(p.days[1], 20.0));
        assert!(close(p.days[2], 30.0));
        assert!(close(p.days.iter().sum::<f64>(), 60.0));
        assert_eq!(p.today, Some(2));
        assert!(close(p.today_spent().unwrap(), 30.0));
    }

    #[test]
    fn avg_and_budget_from_elapsed_and_remaining() {
        // 2 days in, 40% used → 20%/day; 60% over 5 days left → 12%/day.
        let p = daily_pace(START, START + WEEK, START + 2 * DAY_SECS, 40.0, &[1; 7]);
        assert!(close(p.avg_per_day, 20.0));
        assert!(close(p.budget_per_day.unwrap(), 12.0));
    }

    #[test]
    fn today_budget_is_fixed_at_slice_start() {
        // Day 3 (4 slices left incl. today). 50% used before today, 10% today:
        // today's budget = (100 - 50) / 4 = 12.5%, regardless of today's spend.
        let p = daily_pace(
            START,
            START + WEEK,
            START + 3 * DAY_SECS + 7200,
            60.0,
            &[10, 20, 20, 10, 0, 0, 0],
        );
        assert_eq!(p.today, Some(3));
        assert!(close(p.today_spent().unwrap(), 10.0));
        assert!(close(p.today_budget.unwrap(), 12.5));
    }

    #[test]
    fn over_quota_budget_is_zero() {
        let p = daily_pace(START, START + WEEK, START + DAY_SECS, 120.0, &[1; 7]);
        assert!(close(p.budget_per_day.unwrap(), 0.0));
    }

    #[test]
    fn closed_window_has_no_budget() {
        let p = daily_pace(START, START + WEEK, START + WEEK, 70.0, &[1; 7]);
        assert_eq!(p.today, None);
        assert_eq!(p.budget_per_day, None);
        assert_eq!(p.today_budget, None);
        assert!(close(p.avg_per_day, 10.0));
    }

    #[test]
    fn no_output_means_empty_days_but_real_average() {
        let p = daily_pace(START, START + WEEK, START + DAY_SECS, 14.0, &[0; 7]);
        assert!(p.days.iter().all(|&d| d == 0.0));
        assert!(close(p.avg_per_day, 14.0));
    }

    #[test]
    fn first_minutes_do_not_explode_the_average() {
        // 1% in the first minute averages over the 1 h floor, not 1 minute.
        let p = daily_pace(START, START + WEEK, START + 60, 1.0, &[1; 7]);
        assert!(close(p.avg_per_day, 24.0));
    }

    #[test]
    fn abnormal_used_pct_reads_as_zero() {
        for pct in [f64::NAN, f64::INFINITY, -5.0] {
            let p = daily_pace(START, START + WEEK, START + DAY_SECS, pct, &[1; 7]);
            assert!(p.avg_per_day.is_finite());
            assert!(p.days.iter().all(|d| d.is_finite()));
        }
    }

    #[test]
    fn empty_slices_never_panic() {
        let p = daily_pace(START, START, START, 50.0, &[]);
        assert_eq!(p.today, None);
        assert!(p.days.is_empty());
    }
}
