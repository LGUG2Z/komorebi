use std::sync::Arc;

use parking_lot::Mutex;
use parking_lot::RwLock;

use crate::Settings;

/// The current [`Settings`] of a daemon.
///
/// Reading costs one uncontended read lock and an `Arc` clone; the lock is held for the clone
/// only, never while the caller uses the snapshot. The representation (an `RwLock<Arc<_>>`) is
/// private: it can become an `ArcSwap` later without touching a caller.
pub struct Handle<P> {
    current: RwLock<Arc<Settings<P>>>,
    // Serialises writers. Readers never take it.
    writers: Mutex<()>,
}

impl<P> Handle<P> {
    pub fn new(settings: Settings<P>) -> Self {
        Self {
            current: RwLock::new(Arc::new(settings)),
            writers: Mutex::new(()),
        }
    }

    /// The settings as they are now. Take it once per unit of work (one border render, one
    /// layout pass, one rule check) and use it for all of it.
    pub fn snapshot(&self) -> Arc<Settings<P>> {
        Arc::clone(&self.current.read())
    }

    /// Publish a whole new value. Readers that already hold a snapshot keep the old one.
    pub fn replace(&self, settings: Settings<P>) {
        let _writer = self.writers.lock();
        *self.current.write() = Arc::new(settings);
    }
}

impl<P: Clone> Handle<P> {
    /// Copy the current value, let `edit` change the copy and publish it once.
    ///
    /// Writers are serialised for the whole call, so an `update` never overwrites a change made by
    /// another `update` that ran at the same time. If `edit` panics nothing is published.
    pub fn update<R>(&self, edit: impl FnOnce(&mut Settings<P>) -> R) -> R {
        let _writer = self.writers.lock();
        let mut copy = (**self.current.read()).clone();
        let result = edit(&mut copy);
        *self.current.write() = Arc::new(copy);
        result
    }
}

impl<P: Clone + 'static> Handle<P> {
    /// Set one field now and put its previous value back when the guard is dropped.
    ///
    /// The previous value is restored only if the field still holds the value this call set. If a
    /// reload or a command changed the field meanwhile, that change wins. (The statics did the
    /// opposite: the saved value overwrote whatever had been written in between.)
    pub fn scoped<T>(&self, field: fn(&mut Settings<P>) -> &mut T, value: T) -> Scoped<'_, P, T>
    where
        T: Clone + PartialEq,
    {
        let previous = self.update(|settings| std::mem::replace(field(settings), value.clone()));

        Scoped {
            handle: self,
            field,
            set: value,
            previous: Some(previous),
        }
    }
}

/// The guard returned by [`Handle::scoped`].
#[must_use = "the previous value is put back when the guard is dropped"]
pub struct Scoped<'a, P: Clone + 'static, T: Clone + PartialEq> {
    handle: &'a Handle<P>,
    field: fn(&mut Settings<P>) -> &mut T,
    set: T,
    previous: Option<T>,
}

impl<P: Clone + 'static, T: Clone + PartialEq> Drop for Scoped<'_, P, T> {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            let field = self.field;
            let set = &self.set;
            self.handle.update(|settings| {
                let slot = field(settings);
                if slot == set {
                    *slot = previous;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::PlatformDefaults;

    #[derive(Clone, Debug, PartialEq, Default)]
    struct Platform {
        hiding: u8,
        flag: bool,
    }

    fn handle() -> Handle<Platform> {
        Handle::new(Settings::new(
            crate::Shared::defaults_for(PlatformDefaults::KDE),
            Platform::default(),
        ))
    }

    #[test]
    fn a_snapshot_does_not_change_when_the_handle_does() {
        let handle = handle();
        let before = handle.snapshot();

        handle.update(|s| s.shared.border.width = 99);

        assert_eq!(before.shared.border.width, 10);
        assert_eq!(handle.snapshot().shared.border.width, 99);
    }

    #[test]
    fn replace_publishes_the_whole_value() {
        let handle = handle();
        let mut next = (*handle.snapshot()).clone();
        next.platform.hiding = 3;
        next.shared.border.enabled = false;

        handle.replace(next);

        let now = handle.snapshot();
        assert_eq!(now.platform.hiding, 3);
        assert!(!now.shared.border.enabled);
    }

    #[test]
    fn a_panicking_edit_publishes_nothing() {
        let handle = handle();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle.update(|s| {
                s.shared.border.width = 77;
                panic!("edit failed");
            });
        }));

        assert!(result.is_err());
        assert_eq!(handle.snapshot().shared.border.width, 10);
        // and the writer lock is free again
        handle.update(|s| s.shared.border.width = 5);
        assert_eq!(handle.snapshot().shared.border.width, 5);
    }

    #[test]
    fn concurrent_updates_lose_nothing() {
        let handle = Arc::new(handle());
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let handle = Arc::clone(&handle);
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        handle.update(|s| s.shared.border.width += 1);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(handle.snapshot().shared.border.width, 10 + 8 * 500);
    }

    #[test]
    fn readers_never_see_half_an_update() {
        // The writer keeps width and offset equal; a torn read would show them different.
        let handle = Arc::new(handle());
        handle.update(|s| {
            s.shared.border.width = 0;
            s.shared.border.offset = 0;
        });
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let writer = {
            let (handle, stop) = (Arc::clone(&handle), Arc::clone(&stop));
            std::thread::spawn(move || {
                let mut n = 0;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    n += 1;
                    handle.update(|s| {
                        s.shared.border.width = n;
                        s.shared.border.offset = n;
                    });
                }
            })
        };

        for _ in 0..50_000 {
            let seen = handle.snapshot();
            assert_eq!(seen.shared.border.width, seen.shared.border.offset);
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        writer.join().unwrap();
    }

    #[test]
    fn scoped_puts_the_value_back() {
        let handle = handle();

        {
            let _guard = handle.scoped(|s| &mut s.platform.hiding, 9);
            assert_eq!(handle.snapshot().platform.hiding, 9);
        }

        assert_eq!(handle.snapshot().platform.hiding, 0);
    }

    #[test]
    fn scoped_leaves_a_change_made_meanwhile() {
        let handle = handle();

        {
            let _guard = handle.scoped(|s| &mut s.platform.hiding, 9);
            // a reload writes the field while the override is held
            handle.update(|s| s.platform.hiding = 4);
        }

        assert_eq!(handle.snapshot().platform.hiding, 4);
    }

    #[test]
    fn scoped_touches_only_its_own_field() {
        let handle = handle();

        {
            let _guard = handle.scoped(|s| &mut s.platform.flag, true);
            handle.update(|s| s.shared.border.width = 31);
        }

        let now = handle.snapshot();
        assert!(!now.platform.flag);
        assert_eq!(now.shared.border.width, 31);
    }

    #[test]
    fn nested_scopes_unwind_in_order() {
        let handle = handle();

        let outer = handle.scoped(|s| &mut s.platform.hiding, 1);
        let inner = handle.scoped(|s| &mut s.platform.hiding, 2);
        assert_eq!(handle.snapshot().platform.hiding, 2);
        drop(inner);
        assert_eq!(handle.snapshot().platform.hiding, 1);
        drop(outer);
        assert_eq!(handle.snapshot().platform.hiding, 0);
    }
}
