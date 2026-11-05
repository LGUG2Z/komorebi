//! The master clock on Linux: `CLOCK_MONOTONIC`, the clock PipeWire stamps its buffers with and
//! the journal's `__MONOTONIC_TIMESTAMP` is on.
//!
//! Every time in a session is nanoseconds on this clock (see [`crate::clock`]). It does not count
//! the time the machine is suspended, so a sleep shows up as the wall clock moving on while this
//! one does not. [`reference_clocks`] reads `CLOCK_BOOTTIME`, which does count it.
//!
//! The same items exist on macOS and Windows.

use crate::clock::Timebase;
use crate::platform::ClockKind;

/// Which clock the master clock is: [`ClockKind::Monotonic`], recorded as `host.clock` in the
/// manifest.
pub const KIND: ClockKind = ClockKind::Monotonic;

/// Returns the identity timebase: [`now_ns`] is already in nanoseconds.
#[must_use]
pub fn timebase() -> Timebase {
    Timebase { numer: 1, denom: 1 }
}

fn read(clock: libc::clockid_t) -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid out pointer for the duration of the call
    let status = unsafe { libc::clock_gettime(clock, &raw mut ts) };
    if status != 0 {
        return 0;
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Returns the raw `CLOCK_MONOTONIC` reading in nanoseconds, which pauses while the machine is
/// suspended.
#[must_use]
pub fn now_ticks() -> u64 {
    read(libc::CLOCK_MONOTONIC)
}

/// Returns nanoseconds on the host monotonic clock: the master clock every record in a session is
/// on.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::platform::clock::now_ns;
///
/// let first = now_ns();
/// assert!(first > 0);
/// assert!(now_ns() >= first);
/// ```
#[must_use]
pub fn now_ns() -> u64 {
    timebase().ticks_to_ns(now_ticks())
}

/// Returns `CLOCK_BOOTTIME`, which counts the time the machine is suspended.
#[must_use]
pub fn reference_clocks() -> Vec<crate::clock::ReferenceClock> {
    vec![crate::clock::ReferenceClock {
        name: "CLOCK_BOOTTIME".to_string(),
        counts_sleep: true,
        ns: read(libc::CLOCK_BOOTTIME),
    }]
}
