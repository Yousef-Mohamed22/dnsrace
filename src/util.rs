//! Small shared helpers: a tiny random number generator, statistics and time conversion.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static RNG: AtomicU64 = AtomicU64::new(0x2545_F491_4F6C_DD1D);

/// Seeds the generator from the clock and the process id.
pub fn seed_rng() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    RNG.store(
        nanos ^ u64::from(std::process::id()).rotate_left(32),
        Ordering::Relaxed,
    );
}

/// splitmix64 over an atomic counter: plenty for DNS transaction IDs and throw-away labels.
pub fn rand64() -> u64 {
    let mut z = RNG
        .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A duration in milliseconds, with sub-millisecond precision.
pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The median of the values (sorts the slice). `None` when it is empty.
pub fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

/// A nearest-rank percentile (sorts the slice). `p` is clamped to 0-100. `None` when empty.
pub fn percentile(v: &mut [f64], p: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let rank = ((p.clamp(0.0, 100.0) / 100.0) * v.len() as f64)
        .ceil()
        .max(1.0) as usize;
    Some(v[rank.min(v.len()) - 1])
}

/// Population standard deviation: a simple measure of how much lookup times jump around.
/// `None` when the slice is empty.
pub fn stddev(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let variance = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64;
    Some(variance.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_of_nothing_one_odd_and_even() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [7.0]), Some(7.0));
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        assert_eq!(percentile(&mut [], 95.0), None);
        assert_eq!(percentile(&mut [5.0], 95.0), Some(5.0));
        let mut v: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(percentile(&mut v, 0.0), Some(1.0));
        assert_eq!(percentile(&mut v, 50.0), Some(10.0));
        assert_eq!(percentile(&mut v, 95.0), Some(19.0));
        assert_eq!(percentile(&mut v, 100.0), Some(20.0));
        assert_eq!(percentile(&mut v, 999.0), Some(20.0));
    }

    #[test]
    fn stddev_measures_spread() {
        assert_eq!(stddev(&[]), None);
        assert_eq!(stddev(&[7.0]), Some(0.0));
        assert_eq!(stddev(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]), Some(2.0));
    }

    #[test]
    fn ms_converts_with_sub_millisecond_precision() {
        assert_eq!(ms(Duration::from_millis(5)), 5.0);
        assert!((ms(Duration::from_micros(1500)) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn rand64_does_not_repeat() {
        seed_rng();
        let values: std::collections::HashSet<u64> = (0..1000).map(|_| rand64()).collect();
        assert_eq!(values.len(), 1000);
    }
}
