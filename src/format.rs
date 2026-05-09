use std::sync::OnceLock;

use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

static LOCAL_OFFSET: OnceLock<UtcOffset> = OnceLock::new();

/// Captures the local UTC offset and caches it. Must be called from a
/// single-threaded context — `UtcOffset::current_local_offset` is unsound
/// once other threads exist (libc `localtime_r` race with `setenv`).
pub fn init_local_offset() -> UtcOffset {
    *LOCAL_OFFSET.get_or_init(|| UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC))
}

fn local_offset() -> UtcOffset {
    LOCAL_OFFSET.get().copied().unwrap_or(UtcOffset::UTC)
}

/// `42`, `1.2k`, `12k`, `1.4M`, `42M`.
pub fn compact(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 10_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else if n < 1_000_000 {
        format!("{}k", n / 1_000)
    } else if n < 10_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else {
        format!("{}M", n / 1_000_000)
    }
}

/// `HH:MM:SS` from a duration in seconds. Negatives clamp to zero.
pub fn duration_hms(seconds: i64) -> String {
    let s = seconds.max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    format!("{h:02}:{m:02}:{sec:02}")
}

/// Local-timezone `HH:MM:SS` for an epoch-seconds instant.
/// Returns `??:??:??` if the input is out of range.
pub fn iso_local_hms(epoch_secs: i64) -> String {
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(epoch_secs) else {
        return "??:??:??".into();
    };
    let fmt = format_description!("[hour]:[minute]:[second]");
    dt.to_offset(local_offset())
        .format(&fmt)
        .unwrap_or_else(|_| "??:??:??".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_under_1k() {
        assert_eq!(compact(0), "0");
        assert_eq!(compact(42), "42");
        assert_eq!(compact(999), "999");
    }

    #[test]
    fn compact_1k_to_10k_one_decimal() {
        assert_eq!(compact(1_000), "1.0k");
        assert_eq!(compact(1_200), "1.2k");
        assert_eq!(compact(7_500), "7.5k");
    }

    #[test]
    fn compact_10k_to_1m_no_decimal() {
        assert_eq!(compact(10_000), "10k");
        assert_eq!(compact(42_700), "42k");
        assert_eq!(compact(999_999), "999k");
    }

    #[test]
    fn compact_1m_to_10m_one_decimal() {
        assert_eq!(compact(1_000_000), "1.0M");
        assert_eq!(compact(1_400_000), "1.4M");
        assert_eq!(compact(9_999_999), "10.0M");
    }

    #[test]
    fn compact_above_10m() {
        assert_eq!(compact(10_000_000), "10M");
        assert_eq!(compact(99_999_999), "99M");
    }

    #[test]
    fn duration_hms_zero_and_negative() {
        assert_eq!(duration_hms(0), "00:00:00");
        assert_eq!(duration_hms(-5), "00:00:00");
    }

    #[test]
    fn duration_hms_typical() {
        assert_eq!(duration_hms(59), "00:00:59");
        assert_eq!(duration_hms(60), "00:01:00");
        assert_eq!(duration_hms(3661), "01:01:01");
        assert_eq!(duration_hms(8094), "02:14:54");
    }

    #[test]
    fn iso_local_hms_invalid_returns_placeholder() {
        assert_eq!(iso_local_hms(i64::MAX), "??:??:??");
    }

    #[test]
    fn iso_local_hms_format_shape() {
        let s = iso_local_hms(1_778_377_597);
        assert_eq!(s.len(), 8);
        let bytes = s.as_bytes();
        assert_eq!(bytes[2], b':');
        assert_eq!(bytes[5], b':');
    }
}
