//! A record of how an animation burst went, logged once the animator goes idle
//!
//! Smoothness is decided by two things: how regularly frames are produced, and how long the apps take
//! to apply them. This keeps just enough of both to answer "why did that feel slow" from the log.

use parking_lot::Mutex;
use std::sync::LazyLock;
use std::time::Duration;
use std::time::Instant;

/// How many write durations are kept for one burst
const MAX_SAMPLES: usize = 8192;

#[derive(Debug, Default)]
struct Burst {
    started: Option<Instant>,
    ticks: u64,
    overruns: u64,
    windows_moved: u64,
    writes: u64,
    failed_writes: u64,
    write_micros: Vec<u64>,
}

static BURST: LazyLock<Mutex<Burst>> = LazyLock::new(|| Mutex::new(Burst::default()));

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub wall: Duration,
    pub ticks: u64,
    pub overruns: u64,
    pub windows_moved: u64,
    pub writes: u64,
    pub failed_writes: u64,
    pub write_p50: Duration,
    pub write_p95: Duration,
    pub write_max: Duration,
}

impl Summary {
    /// How many frames per second the animator actually produced
    pub fn effective_fps(&self) -> f64 {
        let seconds = self.wall.as_secs_f64();

        if seconds <= 0.0 {
            return 0.0;
        }

        self.ticks as f64 / seconds
    }
}

/// A frame was produced
pub fn record_tick(now: Instant, windows_moved: usize, overran: bool) {
    let mut burst = BURST.lock();
    burst.started.get_or_insert(now);
    burst.ticks += 1;
    burst.windows_moved += windows_moved as u64;

    if overran {
        burst.overruns += 1;
    }
}

/// A window was written to
pub fn record_write(took: Duration, succeeded: bool) {
    let mut burst = BURST.lock();
    burst.writes += 1;

    if !succeeded {
        burst.failed_writes += 1;
    }

    if burst.write_micros.len() < MAX_SAMPLES {
        burst.write_micros.push(took.as_micros() as u64);
    }
}

/// Close the current burst and return what happened in it, if anything did
pub fn take_summary(now: Instant) -> Option<Summary> {
    let mut burst = BURST.lock();
    let finished = std::mem::take(&mut *burst);
    drop(burst);

    let started = finished.started?;

    let mut micros = finished.write_micros;
    micros.sort_unstable();

    Some(Summary {
        wall: now.saturating_duration_since(started),
        ticks: finished.ticks,
        overruns: finished.overruns,
        windows_moved: finished.windows_moved,
        writes: finished.writes,
        failed_writes: finished.failed_writes,
        write_p50: percentile(&micros, 50),
        write_p95: percentile(&micros, 95),
        write_max: micros
            .last()
            .copied()
            .map(Duration::from_micros)
            .unwrap_or_default(),
    })
}

/// The value at `percent` of the way through already sorted samples
fn percentile(sorted: &[u64], percent: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }

    let rank = (sorted.len() * percent).div_ceil(100).max(1);

    Duration::from_micros(sorted[(rank - 1).min(sorted.len() - 1)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_of_nothing_are_zero() {
        assert_eq!(percentile(&[], 50), Duration::ZERO);
    }

    #[test]
    fn percentiles_pick_the_expected_samples() {
        let samples = (1..=100).collect::<Vec<u64>>();

        assert_eq!(percentile(&samples, 50), Duration::from_micros(50));
        assert_eq!(percentile(&samples, 95), Duration::from_micros(95));
        assert_eq!(percentile(&samples, 100), Duration::from_micros(100));
        assert_eq!(percentile(&[7], 95), Duration::from_micros(7));
    }

    #[test]
    fn effective_fps_is_ticks_over_wall_time() {
        let summary = Summary {
            wall: Duration::from_millis(500),
            ticks: 30,
            overruns: 0,
            windows_moved: 90,
            writes: 90,
            failed_writes: 0,
            write_p50: Duration::ZERO,
            write_p95: Duration::ZERO,
            write_max: Duration::ZERO,
        };

        assert!((summary.effective_fps() - 60.0).abs() < 1e-9);
    }
}
