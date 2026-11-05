//! The master clock on macOS: `mach_absolute_time`, the clock `ScreenCaptureKit` and the unified
//! log already use.
//!
//! Every time in a session is nanoseconds on this clock (see [`crate::clock`]). It counts ticks of
//! a size the OS reports ([`timebase`]) and pauses while the machine sleeps, so a sleep shows up as
//! the wall clock moving on while this one does not. [`reference_clocks`] reads the clock that does
//! keep counting through a sleep.
//!
//! The same items exist on Windows, where the clock is `QueryPerformanceCounter`.

use crate::clock::Timebase;
use crate::platform::ClockKind;
use std::sync::OnceLock;

/// Which clock the master clock is: [`ClockKind::Mach`], recorded as `host.clock` in the manifest.
pub const KIND: ClockKind = ClockKind::Mach;

#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

// `libc` marks its mach bindings deprecated in favour of the `mach2` crate; these two symbols
// are all we need from libSystem, so declare them directly instead of adding a dependency
unsafe extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_continuous_time() -> u64;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
}

/// Returns what one tick of [`now_ticks`] is worth, as `numer / denom` nanoseconds.
///
/// It is read from `mach_timebase_info` once and kept. If the OS reports no usable timebase (which
/// a Mac that can run this always does), it is `1 / 1`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::platform::clock::{now_ns, now_ticks, timebase};
///
/// // the master clock is the tick count scaled by the timebase, and never goes backwards
/// let ns = timebase().ticks_to_ns(now_ticks());
/// assert!(now_ns() >= ns);
/// ```
#[must_use]
pub fn timebase() -> Timebase {
    static TIMEBASE: OnceLock<Timebase> = OnceLock::new();
    *TIMEBASE.get_or_init(|| {
        let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
        // SAFETY: `info` is a valid out pointer for the duration of the call
        let status = unsafe { mach_timebase_info(&raw mut info) };
        if status != 0 || info.denom == 0 {
            // every Mac that can run this has a valid timebase; 1/1 is the safe identity
            return Timebase { numer: 1, denom: 1 };
        }

        Timebase {
            numer: info.numer,
            denom: info.denom,
        }
    })
}

/// Returns the raw tick count of `mach_absolute_time`, which pauses while the machine sleeps.
#[must_use]
pub fn now_ticks() -> u64 {
    // SAFETY: no preconditions
    unsafe { mach_absolute_time() }
}

/// Returns nanoseconds on the host monotonic clock: the master clock every record in a session is
/// on. It is [`now_ticks`] scaled by [`timebase`], so it pauses while the machine sleeps.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::platform::clock::now_ns;
///
/// let first = now_ns();
/// let second = now_ns();
/// assert!(second >= first);
/// ```
#[must_use]
pub fn now_ns() -> u64 {
    timebase().ticks_to_ns(now_ticks())
}

/// Returns the host's other clocks, to be read beside the master clock: one,
/// `mach_continuous_time`, which counts the time the machine spends asleep, unlike the master
/// clock.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::platform::clock::reference_clocks;
///
/// let clocks = reference_clocks();
/// assert_eq!(clocks.len(), 1);
/// assert_eq!(clocks[0].name, "mach_continuous_time");
/// assert!(clocks[0].counts_sleep);
/// ```
#[must_use]
pub fn reference_clocks() -> Vec<crate::clock::ReferenceClock> {
    // SAFETY: no preconditions
    let ticks = unsafe { mach_continuous_time() };
    vec![crate::clock::ReferenceClock {
        name: "mach_continuous_time".to_string(),
        counts_sleep: true,
        ns: timebase().ticks_to_ns(ticks),
    }]
}
