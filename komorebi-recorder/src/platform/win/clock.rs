//! The master clock on Windows: `QueryPerformanceCounter`, the clock Windows.Graphics.Capture frame
//! times are on.
//!
//! Every time in a session is nanoseconds on this clock (see [`crate::clock`]). It counts ticks of
//! a size the OS reports ([`timebase`]). Whether it keeps counting while the machine sleeps is not
//! assumed: `doctor --probe clock` measures it, using the two clocks of [`reference_clocks`], and
//! the power watcher (`crate::platform::power`) says when a sleep happens either way.
//!
//! The same items exist on macOS, where the clock is `mach_absolute_time`.

use crate::clock::Timebase;
use crate::platform::ClockKind;
use std::sync::OnceLock;
use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::Performance::QueryPerformanceFrequency;
use windows::Win32::System::WindowsProgramming::QueryInterruptTime;
use windows::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTimePrecise;

/// Which clock the master clock is: [`ClockKind::Qpc`], recorded as `host.clock` in the manifest.
pub const KIND: ClockKind = ClockKind::Qpc;

/// Returns what one tick of [`now_ticks`] is worth, as `numer / denom` nanoseconds.
///
/// It is made from `QueryPerformanceFrequency` once and kept. If that cannot be read, which it
/// cannot do on any Windows that runs this, the frequency is taken to be 10 MHz, which is what it
/// is nearly everywhere.
#[must_use]
pub fn timebase() -> Timebase {
    static TIMEBASE: OnceLock<Timebase> = OnceLock::new();
    *TIMEBASE.get_or_init(|| {
        let mut frequency = 0_i64;
        // SAFETY: `frequency` is a valid out pointer for the duration of the call
        let known = unsafe { QueryPerformanceFrequency(&raw mut frequency) }.is_ok();
        // it cannot fail on any Windows that runs this; 10 MHz is what it says nearly everywhere
        Timebase::from_frequency(if known && frequency > 0 {
            frequency as u64
        } else {
            10_000_000
        })
    })
}

/// Returns the raw `QueryPerformanceCounter` tick count. A negative reading, which does not
/// happen, is `0`.
#[must_use]
pub fn now_ticks() -> u64 {
    let mut ticks = 0_i64;
    // SAFETY: `ticks` is a valid out pointer for the duration of the call
    let _ = unsafe { QueryPerformanceCounter(&raw mut ticks) };
    ticks.max(0) as u64
}

/// Returns nanoseconds on the host monotonic clock: the master clock every record in a session is
/// on. It is [`now_ticks`] scaled by [`timebase`].
#[must_use]
pub fn now_ns() -> u64 {
    timebase().ticks_to_ns(now_ticks())
}

/// Returns the host's other clocks, to be read beside the master clock: the two interrupt-time
/// clocks.
///
/// `QueryInterruptTime` counts the time the machine is suspended and
/// `QueryUnbiasedInterruptTimePrecise` does not. Both are read in 100 ns units and returned in
/// nanoseconds. Which of them the performance counter behaves like is what decides how a sleep
/// can be noticed.
#[must_use]
pub fn reference_clocks() -> Vec<crate::clock::ReferenceClock> {
    // SAFETY: no preconditions
    let (biased, unbiased) = unsafe { (QueryInterruptTime(), QueryUnbiasedInterruptTimePrecise()) };
    vec![
        crate::clock::ReferenceClock {
            name: "QueryInterruptTime".to_string(),
            counts_sleep: true,
            ns: biased.saturating_mul(100),
        },
        crate::clock::ReferenceClock {
            name: "QueryUnbiasedInterruptTimePrecise".to_string(),
            counts_sleep: false,
            ns: unbiased.saturating_mul(100),
        },
    ]
}
