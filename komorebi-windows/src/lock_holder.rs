//! Who holds the window manager lock, and for how long.
//!
//! The window manager is one `Arc<Mutex<WindowManager>>` that every thread locks. When a hold
//! runs long, the other threads can only say that they are waiting ("could not acquire", "stuck
//! for"), not for whom. A place that locks it calls [`held`] right after it has the guard and
//! keeps the returned [`Held`] next to the guard: the holder is recorded in a slot of its own
//! (a mutex that is only ever held for the time of a copy, never while anything else is done),
//! a hold that lasts longer than [`SLOW`] is logged when it ends, and [`describe`] tells the
//! waiters who it is.

use parking_lot::Mutex;
use std::borrow::Cow;
use std::thread::Thread;
use std::time::Duration;
use std::time::Instant;

/// A hold longer than this is logged when it ends
pub const SLOW: Duration = Duration::from_millis(200);

struct Holder {
    what: Cow<'static, str>,
    thread: Thread,
    since: Instant,
}

static HOLDER: Mutex<Option<Holder>> = Mutex::new(None);

/// Records the calling thread as the holder of the window manager lock, until it is dropped
#[must_use = "the hold ends when this is dropped: keep it next to the lock guard"]
pub struct Held {
    what: Cow<'static, str>,
    thread: Thread,
    since: Instant,
}

/// Says who has just locked the window manager: `what` is the call site or the command
pub fn held(what: impl Into<Cow<'static, str>>) -> Held {
    let what = what.into();
    let thread = std::thread::current();
    let since = Instant::now();

    *HOLDER.lock() = Some(Holder {
        what: what.clone(),
        thread: thread.clone(),
        since,
    });

    Held {
        what,
        thread,
        since,
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let elapsed = self.since.elapsed();

        // only clear the slot if it is still this hold's (another thread may have locked and
        // recorded itself in the instant between this guard's release and this drop)
        {
            let mut slot = HOLDER.lock();
            if slot
                .as_ref()
                .is_some_and(|holder| holder.since == self.since)
            {
                *slot = None;
            }
        }

        if elapsed >= SLOW {
            tracing::warn!(
                "window manager lock held by {} (thread {}) for {} ms",
                self.what,
                self.thread.name().unwrap_or("unnamed"),
                elapsed.as_millis()
            );
        }
    }
}

/// Who holds the lock now and for how long so far, for the warnings of the threads that wait
pub fn describe() -> Option<String> {
    HOLDER.lock().as_ref().map(|holder| {
        format!(
            "{} (thread {}) for {} ms so far",
            holder.what,
            holder.thread.name().unwrap_or("unnamed"),
            holder.since.elapsed().as_millis()
        )
    })
}

/// A lock guard together with the record of who holds it, released together: the record first, then
/// the lock, so the diagnostics never say that a thread holds the lock after it has let go. Drop it
/// (`drop(guard)`) at the point where the lock is released.
pub struct HeldGuard<'a, T> {
    // field order is drop order: the record goes before the guard
    _held: Held,
    guard: parking_lot::MutexGuard<'a, T>,
}

/// Locks `mutex` and records `what` as the holder until the returned guard is dropped
pub fn lock<T>(mutex: &Mutex<T>, what: impl Into<Cow<'static, str>>) -> HeldGuard<'_, T> {
    let guard = mutex.lock();
    HeldGuard {
        _held: held(what),
        guard,
    }
}

impl<T> std::ops::Deref for HeldGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> std::ops::DerefMut for HeldGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_holder_is_described_while_held_and_not_after() {
        let hold = held("a test");
        let described = describe().expect("a holder is recorded");
        assert!(described.starts_with("a test (thread "), "{described}");
        drop(hold);

        // another test may hold at the same time: only check that this one is gone
        assert!(describe().is_none_or(|d| !d.starts_with("a test (")));
    }
}
