//! A thread-safe table from a [`WindowId`] to whatever a platform needs in order to act on that
//! window from nothing but the id.
//!
//! macOS is the platform that needs it: a window there is an Accessibility element with the pid
//! of its application and the observer that reports its events, and nothing can be done to a
//! window without them. The table is generic over the handle (`H`) so that its rules can be
//! tested on any host with a stand-in handle; macOS instantiates it with its Accessibility types.
//!
//! # Rules
//!
//! 1. **Thread-safe.** Any thread may call any method.
//! 2. **Nothing of the platform runs under the lock.** [`Registry::get`] clones the handle (the
//!    handle must be cheap to clone: a retained reference) and returns; the caller acts on the
//!    clone with the lock released. A method that removes an entry hands the removed handle back
//!    to the caller instead of dropping it inside, because dropping a macOS handle can make an
//!    Accessibility round trip. This is what keeps a registry call from ever waiting on the main
//!    queue, or on an application, while holding the lock (`docs/dev/seam.md` section 3.3).
//! 3. **A stale id is an error, not a panic.** Looking up an id that was never registered, was
//!    removed, or was removed with its application answers [`Error::WindowGone`].
//! 4. **Entries end** when the window is destroyed ([`Registry::remove`]), when its application
//!    quits ([`Registry::remove_pid`]), and when a call on the window shows that it is gone. An
//!    entry that nobody removes is a leak of a retained element; the platform removes at every
//!    place it learns of a death.
//! 5. **Re-registering an id replaces the entry** and returns the old handle (a tab that is
//!    now shown by another element keeps the window id).
//!
//! ```
//! use komorebi_backend::Registry;
//! use komorebi_backend::WindowId;
//!
//! let registry: Registry<&'static str> = Registry::new();
//! let id = WindowId::from_u32(7);
//! registry.insert(id, 100, "element");
//! assert_eq!(registry.get(id).unwrap(), "element");
//!
//! registry.remove(id);
//! assert!(registry.get(id).unwrap_err().is_window_gone());
//! ```

use crate::Error;
use crate::Result;
use crate::WindowId;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::MutexGuard;

struct Entry<H> {
    pid: i32,
    handle: H,
}

/// The table. See the [module documentation](self) for its rules.
pub struct Registry<H> {
    entries: Mutex<HashMap<WindowId, Entry<H>>>,
}

impl<H> Default for Registry<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H> Registry<H> {
    /// Returns an empty registry.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    // a thread that panicked while holding the lock cannot have left an entry half written (every
    // method is a single map operation), so a poisoned lock is still good
    fn lock(&self) -> MutexGuard<'_, HashMap<WindowId, Entry<H>>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers the window, owned by the process `pid`. Returns the handle that this replaces,
    /// if the id was registered: the caller drops it, with no lock held.
    #[must_use = "drop the replaced handle yourself: it is not dropped under the lock"]
    pub fn insert(&self, id: WindowId, pid: i32, handle: H) -> Option<H> {
        self.lock()
            .insert(id, Entry { pid, handle })
            .map(|e| e.handle)
    }

    /// Replaces the handle of a window that is registered, keeping its pid. Returns the old
    /// handle, or [`Error::WindowGone`] (and registers nothing) if the id is not registered.
    pub fn replace(&self, id: WindowId, handle: H) -> Result<H> {
        match self.lock().get_mut(&id) {
            Some(entry) => Ok(std::mem::replace(&mut entry.handle, handle)),
            None => Err(Error::WindowGone(id)),
        }
    }

    /// Forgets the window. Returns its handle (for the caller to drop with no lock held), or
    /// `None` if it was not registered, which is not an error: a death is usually seen twice.
    #[must_use = "drop the removed handle yourself: it is not dropped under the lock"]
    pub fn remove(&self, id: WindowId) -> Option<H> {
        self.lock().remove(&id).map(|e| e.handle)
    }

    /// Forgets every window of the process, as when its application quits. Returns the ids and
    /// handles removed, sorted by id.
    #[must_use = "drop the removed handles yourself: they are not dropped under the lock"]
    pub fn remove_pid(&self, pid: i32) -> Vec<(WindowId, H)> {
        let mut entries = self.lock();
        let ids: Vec<WindowId> = entries
            .iter()
            .filter(|(_, e)| e.pid == pid)
            .map(|(id, _)| *id)
            .collect();
        let mut removed: Vec<(WindowId, H)> = ids
            .into_iter()
            .filter_map(|id| entries.remove(&id).map(|e| (id, e.handle)))
            .collect();
        drop(entries);
        removed.sort_by_key(|(id, _)| *id);
        removed
    }

    /// Whether the id is registered.
    pub fn contains(&self, id: WindowId) -> bool {
        self.lock().contains_key(&id)
    }

    /// The pid of the registered window, or [`Error::WindowGone`].
    pub fn pid(&self, id: WindowId) -> Result<i32> {
        self.lock()
            .get(&id)
            .map(|e| e.pid)
            .ok_or(Error::WindowGone(id))
    }

    /// How many windows are registered.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether nothing is registered.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// The registered ids, sorted.
    pub fn ids(&self) -> Vec<WindowId> {
        let mut ids: Vec<_> = self.lock().keys().copied().collect();
        ids.sort();
        ids
    }
}

impl<H: Clone> Registry<H> {
    /// A clone of the handle of the window, or [`Error::WindowGone`] for an id that is not
    /// registered. The lock is released before this returns.
    pub fn get(&self, id: WindowId) -> Result<H> {
        self.lock()
            .get(&id)
            .map(|e| e.handle.clone())
            .ok_or(Error::WindowGone(id))
    }

    /// Clones of every handle with their ids, sorted by id.
    pub fn snapshot(&self) -> Vec<(WindowId, H)> {
        let mut all: Vec<_> = self
            .lock()
            .iter()
            .map(|(id, e)| (*id, e.handle.clone()))
            .collect();
        all.sort_by_key(|(id, _)| *id);
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    fn id(n: u32) -> WindowId {
        WindowId::from_u32(n)
    }

    #[test]
    fn a_registered_window_is_found_by_its_id() {
        let r = Registry::new();
        assert!(r.insert(id(1), 10, "a").is_none());
        assert!(matches!(r.get(id(1)), Ok("a")));
        assert!(matches!(r.pid(id(1)), Ok(10)));
        assert!(r.contains(id(1)));
    }

    #[test]
    fn an_unknown_id_is_window_gone_and_not_a_panic() {
        let r: Registry<&str> = Registry::new();
        assert!(matches!(r.get(id(9)), Err(Error::WindowGone(w)) if w == id(9)));
        assert!(matches!(r.pid(id(9)), Err(Error::WindowGone(_))));
        assert!(r.get(id(9)).unwrap_err().is_window_gone());
    }

    #[test]
    fn a_removed_window_is_stale() {
        let r = Registry::new();
        let _ = r.insert(id(1), 10, "a");
        assert_eq!(r.remove(id(1)), Some("a"));
        assert!(r.get(id(1)).unwrap_err().is_window_gone());
        // a death seen twice is not an error
        assert_eq!(r.remove(id(1)), None);
    }

    #[test]
    fn reinserting_replaces_and_hands_back_the_old_handle() {
        let r = Registry::new();
        let _ = r.insert(id(1), 10, "tab one");
        assert_eq!(r.insert(id(1), 10, "tab two"), Some("tab one"));
        assert!(matches!(r.get(id(1)), Ok("tab two")));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn replace_keeps_the_pid_and_refuses_a_stale_id() {
        let r = Registry::new();
        let _ = r.insert(id(1), 10, "a");
        assert!(matches!(r.replace(id(1), "b"), Ok("a")));
        assert!(matches!(r.pid(id(1)), Ok(10)));
        assert!(r.replace(id(2), "c").unwrap_err().is_window_gone());
        assert!(!r.contains(id(2)));
    }

    #[test]
    fn an_application_quitting_takes_all_its_windows_and_only_them() {
        let r = Registry::new();
        let _ = r.insert(id(3), 10, "c");
        let _ = r.insert(id(1), 10, "a");
        let _ = r.insert(id(2), 20, "b");

        assert_eq!(r.remove_pid(10), vec![(id(1), "a"), (id(3), "c")]);
        assert_eq!(r.ids(), vec![id(2)]);
        assert!(r.get(id(1)).unwrap_err().is_window_gone());
        assert_eq!(r.remove_pid(10), vec![]);
    }

    #[test]
    fn snapshot_is_sorted_and_independent_of_later_changes() {
        let r = Registry::new();
        let _ = r.insert(id(2), 1, "b");
        let _ = r.insert(id(1), 1, "a");
        let snap = r.snapshot();
        let _ = r.remove(id(1));
        assert_eq!(snap, vec![(id(1), "a"), (id(2), "b")]);
    }

    /// A handle whose drop would, on macOS, make a round trip: it must not run under the lock.
    struct Probe {
        registry: Arc<Registry<Arc<Probe>>>,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            // would deadlock if the registry dropped the handle while holding its lock
            let _ = self.registry.len();
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn handles_are_never_dropped_under_the_lock() {
        let registry = Arc::new(Registry::new());
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = |_: u32| {
            Arc::new(Probe {
                registry: registry.clone(),
                drops: drops.clone(),
            })
        };

        let _ = registry.insert(id(1), 10, probe(1));
        // the replaced and the removed handles come back to us, so their drop runs here
        drop(registry.insert(id(1), 10, probe(2)));
        drop(registry.remove(id(1)));
        let _ = registry.insert(id(2), 10, probe(3));
        drop(registry.remove_pid(10));

        assert_eq!(drops.load(Ordering::SeqCst), 3);
        assert!(registry.is_empty());
    }

    /// Registering must not run the drop of any handle: macOS handles do Accessibility calls when
    /// they are dropped, and registration happens on the event thread under the window manager lock.
    #[test]
    fn registering_a_new_id_drops_nothing_and_a_replaced_handle_only_when_the_caller_drops_it() {
        let registry = Arc::new(Registry::new());
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = || {
            Arc::new(Probe {
                registry: registry.clone(),
                drops: drops.clone(),
            })
        };

        let replaced = registry.insert(id(1), 10, probe());
        assert!(replaced.is_none());
        assert_eq!(drops.load(Ordering::SeqCst), 0);

        let replaced = registry.insert(id(1), 10, probe());
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "the old handle is handed back, not dropped"
        );

        drop(replaced);
        assert_eq!(drops.load(Ordering::SeqCst), 1);

        // a lookup clones the Arc and drops nothing
        let found = registry.get(id(1)).unwrap();
        drop(found);
        assert_eq!(drops.load(Ordering::SeqCst), 1);

        drop(registry.remove(id(1)));
        assert_eq!(drops.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn many_threads_can_register_look_up_and_remove() {
        let r = Arc::new(Registry::new());
        let threads: Vec<_> = (0..8u32)
            .map(|t| {
                let r = r.clone();
                std::thread::spawn(move || {
                    for n in 0..200u32 {
                        let wid = id(t * 1000 + n);
                        let _ = r.insert(wid, t as i32, n);
                        assert!(matches!(r.get(wid), Ok(v) if v == n));
                        assert_eq!(r.remove(wid), Some(n));
                        assert!(r.get(wid).unwrap_err().is_window_gone());
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().unwrap();
        }

        assert!(r.is_empty());
    }
}
