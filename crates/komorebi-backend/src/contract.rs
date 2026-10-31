//! The checks that every [`Backend`] has to pass.
//!
//! A platform (or a wrapper around one) implements [`Harness`], which says how to put a window
//! on the backend's desktop, and runs [`check_all`] from a test. Only what the documentation of
//! [`Backend`] promises is checked: nothing here depends on how fast a platform acts, because a
//! write is a request (`set_rect`, `minimize` and the rest return before the platform has
//! acted), so a read after a write is checked for being answerable, not for what it says.
//!
//! [`crate::fake::FakeBackend`] passes them in `tests` below, as the KDE fake does through its
//! adapter, and the adapter of every platform should.
//!
//! # Example
//!
//! ```rust
//! use komorebi_backend::contract::{Harness, check_all};
//! use komorebi_backend::fake::FakeBackend;
//! use komorebi_backend::WindowId;
//!
//! struct Fake(FakeBackend);
//!
//! impl Harness for Fake {
//!     type B = FakeBackend;
//!     fn new() -> Self { Fake(FakeBackend::with_one_monitor()) }
//!     fn backend(&self) -> &FakeBackend { &self.0 }
//!     fn open_window(&self, id: u32) -> WindowId {
//!         self.0.open_window(WindowId::from_u32(id));
//!         WindowId::from_u32(id)
//!     }
//!     fn destroy_window(&self, id: WindowId) { self.0.close_window_now(id) }
//! }
//!
//! check_all::<Fake>();
//! ```

use crate::Backend;
use crate::Batch;
use crate::Capability;
use crate::Error;
use crate::MonitorId;
use crate::Rect;
use crate::WindowId;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;

/// How a test puts windows on the desktop of the backend it checks.
pub trait Harness {
    /// The backend that is checked.
    type B: Backend;

    /// Returns a harness with a fresh desktop that has at least one monitor and no window.
    fn new() -> Self;

    /// Returns the backend that is checked.
    fn backend(&self) -> &Self::B;

    /// Puts a live window on the desktop and returns its id. `seed` is distinct for each call
    /// of one check; a platform may map it to whatever id it uses, and may ignore it.
    fn open_window(&self, seed: u32) -> WindowId;

    /// Destroys the window as its user would, not through the backend.
    fn destroy_window(&self, id: WindowId);
}

/// One check: its name and the function that panics if the backend breaks the contract.
pub type Check = (&'static str, fn());

/// Returns every check, for a harness `H`.
pub fn checks<H: Harness>() -> Vec<Check> {
    vec![
        ("windows are listed by id", windows_are_listed_by_id::<H>),
        (
            "window agrees with windows",
            window_agrees_with_windows::<H>,
        ),
        (
            "an unknown window is not a window",
            an_unknown_window_is_not_a_window::<H>,
        ),
        (
            "window commands on an unknown window say it is gone",
            commands_on_an_unknown_window::<H>,
        ),
        (
            "window commands on a destroyed window say it is gone",
            commands_on_a_destroyed_window::<H>,
        ),
        (
            "a destroyed window is not listed or alive",
            a_destroyed_window_is_not_alive::<H>,
        ),
        (
            "set_rect on a live window succeeds and the frame stays readable",
            set_rect_on_a_live_window::<H>,
        ),
        (
            "hide and restore tolerate repeats",
            hide_and_restore_tolerate_repeats::<H>,
        ),
        (
            "focus names a window the backend knows",
            focus_names_a_known_window::<H>,
        ),
        (
            "capabilities and Unsupported agree",
            capabilities_and_unsupported_agree::<H>,
        ),
        (
            "monitors are sane and found by point",
            monitors_are_sane::<H>,
        ),
        (
            "a batch scope nests and wraps writes",
            a_batch_scope_nests::<H>,
        ),
        (
            "the backend can be called from another thread",
            callable_from_another_thread::<H>,
        ),
        ("the backend is a trait object", is_a_trait_object::<H>),
    ]
}

/// Runs every check and returns the names and messages of the ones that failed.
pub fn run<H: Harness>() -> Vec<String> {
    let mut failures = Vec::new();
    for (name, check) in checks::<H>() {
        if let Err(panic) = catch_unwind(AssertUnwindSafe(check)) {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default();
            failures.push(format!("{name}: {message}"));
        }
    }
    failures
}

/// Runs every check and panics, listing each failed one, if the backend breaks the contract.
pub fn check_all<H: Harness>() {
    let failures = run::<H>();
    assert!(
        failures.is_empty(),
        "contract broken:\n{}",
        failures.join("\n")
    );
}

fn rect(left: i32, width: i32) -> Rect {
    Rect {
        left,
        top: 0,
        right: width,
        bottom: 400,
    }
}

fn gone(result: crate::Result<()>, id: WindowId, what: &str) {
    match result {
        Err(Error::WindowGone(got)) => {
            assert_eq!(got, id, "{what}: the error names another window")
        }
        other => panic!("{what}: expected WindowGone({id}), got {other:?}"),
    }
}

/// Every command about one window, as `(name, call)`.
#[allow(clippy::type_complexity)]
fn window_commands<'a, B: Backend>(
    b: &'a B,
    id: WindowId,
) -> Vec<(&'static str, Box<dyn Fn() -> crate::Result<()> + 'a>)> {
    vec![
        ("set_rect", Box::new(move || b.set_rect(id, &rect(0, 300)))),
        ("focus", Box::new(move || b.focus(id, false))),
        ("focus with the mouse", Box::new(move || b.focus(id, true))),
        ("raise", Box::new(move || b.raise(id))),
        ("hide", Box::new(move || b.hide(id))),
        ("restore", Box::new(move || b.restore(id))),
        ("minimize", Box::new(move || b.minimize(id))),
        ("unminimize", Box::new(move || b.unminimize(id))),
        ("close", Box::new(move || b.close(id))),
    ]
}

fn windows_are_listed_by_id<H: Harness>() {
    let h = H::new();
    let opened: Vec<_> = (1..=3).map(|seed| h.open_window(seed)).collect();

    let listed: Vec<_> = h.backend().windows().iter().map(|w| w.id).collect();

    for id in &opened {
        assert!(listed.contains(id), "window {id} is not listed");
    }
    assert!(
        listed.windows(2).all(|pair| pair[0] < pair[1]),
        "not ordered by id: {listed:?}"
    );
}

fn window_agrees_with_windows<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);

    let info = h.backend().window(id).expect("an open window is known");

    assert_eq!(info.id, id);
    assert!(info.alive);
    assert!(h.backend().is_window(id));
    assert_eq!(h.backend().window_rect(id), Some(info.rect));
    assert_eq!(h.backend().frame(id).ok(), Some(info.rect));
    assert!(h.backend().windows().iter().any(|w| w.id == id));
}

fn an_unknown_window_is_not_a_window<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    let unknown = WindowId(id.0 ^ 0x7fff_ffff);

    assert!(h.backend().window(unknown).is_none());
    assert!(!h.backend().is_window(unknown));
    assert!(h.backend().window_rect(unknown).is_none());
    assert!(
        h.backend()
            .frame(unknown)
            .is_err_and(|e| e.is_window_gone())
    );
}

fn commands_on_an_unknown_window<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    let unknown = WindowId(id.0 ^ 0x7fff_ffff);

    for (name, call) in window_commands(h.backend(), unknown) {
        gone(call(), unknown, name);
    }
}

fn commands_on_a_destroyed_window<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    h.destroy_window(id);

    for (name, call) in window_commands(h.backend(), id) {
        gone(call(), id, name);
    }
}

fn a_destroyed_window_is_not_alive<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    h.destroy_window(id);

    assert!(!h.backend().is_window(id));
    assert!(h.backend().window(id).is_none_or(|w| !w.alive));
    assert!(h.backend().windows().iter().all(|w| w.id != id));
}

fn set_rect_on_a_live_window<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);

    h.backend()
        .set_rect(id, &rect(10, 500))
        .expect("a live window can be asked to move");

    // a request says nothing about the frame the window ends up with, only that it can be read
    assert!(h.backend().window_rect(id).is_some());
    assert!(h.backend().window(id).is_some_and(|w| w.alive));
}

fn hide_and_restore_tolerate_repeats<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    let b = h.backend();

    b.restore(id)
        .expect("restoring a window that is not hidden leaves it as it is");
    b.hide(id).expect("hide");
    b.hide(id)
        .expect("hiding a hidden window leaves it as it is");
    b.restore(id).expect("restore");
    b.restore(id).expect("restore again");
    assert!(b.is_window(id), "hiding is not closing");
}

fn focus_names_a_known_window<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    let b = h.backend();

    b.focus(id, false).expect("focus");
    b.focus(id, true).expect("focus, the pointer following");

    if let Some(foreground) = b.foreground_window() {
        assert!(
            b.window(foreground).is_some(),
            "the foreground window {foreground} is unknown"
        );
    }
    b.center_cursor_in(&rect(0, 200))
        .expect("the pointer can be centred in a rectangle");
}

fn capabilities_and_unsupported_agree<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    let unknown = WindowId(id.0 ^ 0x7fff_ffff);
    let b = h.backend();
    let monitor = b
        .monitors()
        .ok()
        .and_then(|m| m.first().map(|m| m.id))
        .unwrap_or(MonitorId(0));

    for capability in Capability::ALL {
        let calls: Vec<(&str, crate::Result<()>)> = match capability {
            Capability::Workspaces => vec![
                (
                    "ensure_workspaces",
                    b.ensure_workspaces(&["one".to_string()]),
                ),
                ("switch_workspace", b.switch_workspace(monitor, 0)),
                ("set_window_workspace", b.set_window_workspace(id, 0)),
                ("window_workspace", b.window_workspace(id).map(|_| ())),
                ("current_workspaces", b.current_workspaces().map(|_| ())),
            ],
            Capability::Maximize => vec![("maximize", b.maximize(id, false))],
            Capability::Opacity => vec![("set_opacity", b.set_opacity(id, 255))],
            Capability::FocusFollowsMouse => {
                vec![("set_focus_follows_mouse", b.set_focus_follows_mouse(false))]
            }
        };

        for (name, result) in calls {
            let unsupported = matches!(&result, Err(Error::Unsupported(c)) if *c == capability);
            if b.supports(capability) {
                assert!(
                    !unsupported,
                    "{name} is Unsupported, yet {capability:?} is supported"
                );
            } else {
                assert!(
                    unsupported,
                    "{name} must be Unsupported, as {capability:?} is not supported: {result:?}"
                );
            }
        }
    }

    // not supported means Unsupported whatever the window; supported means the window decides
    if !b.supports(Capability::Maximize) {
        assert!(b.maximize(unknown, true).unwrap_err().is_unsupported());
    } else {
        gone(b.maximize(unknown, true), unknown, "maximize");
    }
    if !b.supports(Capability::Opacity) {
        assert!(b.set_opacity(unknown, 10).unwrap_err().is_unsupported());
    } else {
        gone(b.set_opacity(unknown, 10), unknown, "set_opacity");
    }
}

fn monitors_are_sane<H: Harness>() {
    let h = H::new();
    let b = h.backend();

    let monitors = b.monitors().expect("the desktop has a monitor");

    assert!(!monitors.is_empty());
    for m in &monitors {
        assert!(m.size.right > 0 && m.size.bottom > 0, "{m:?} has no size");
        let (l, t) = (m.size.left, m.size.top);
        let w = m.work_area;
        assert!(
            w.left >= l
                && w.top >= t
                && w.left + w.right <= l + m.size.right
                && w.top + w.bottom <= t + m.size.bottom,
            "the work area of {m:?} is not inside the monitor"
        );
        let (cx, cy) = (l + m.size.right / 2, t + m.size.bottom / 2);
        assert!(
            b.monitor_from_point(cx, cy).is_some(),
            "no monitor at the middle of {m:?}"
        );
    }
}

fn a_batch_scope_nests<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    let b = h.backend();

    {
        let _outer = Batch::begin(b);
        {
            let _inner = Batch::begin(b);
            b.set_rect(id, &rect(0, 100))
                .expect("a write inside a batch");
        }
        b.raise(id).expect("a write inside the outer batch");
    }

    b.raise(id).expect("a write after the batch");
}

fn callable_from_another_thread<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    let b = h.backend();

    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                assert!(b.windows().iter().any(|w| w.id == id));
                assert!(b.monitors().is_ok());
                let _ = b.cursor_pos();
                b.set_rect(id, &rect(0, 100))
                    .expect("set_rect from another thread");
            })
            .join()
            .expect("a thread that calls the backend does not panic");
    });
}

fn is_a_trait_object<H: Harness>() {
    let h = H::new();
    let id = h.open_window(1);
    let object: &dyn Backend = h.backend();

    assert!(object.is_window(id));
    let _batch = Batch::begin(object);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeBackend;
    use crate::fake::FrameOffset;

    struct Plain(FakeBackend);
    struct Offset(FakeBackend);
    struct Bare(FakeBackend);

    impl Harness for Plain {
        type B = FakeBackend;
        fn new() -> Self {
            Plain(FakeBackend::with_one_monitor())
        }
        fn backend(&self) -> &FakeBackend {
            &self.0
        }
        fn open_window(&self, seed: u32) -> WindowId {
            self.0.open_window(WindowId::from_u32(seed));
            WindowId::from_u32(seed)
        }
        fn destroy_window(&self, id: WindowId) {
            self.0.close_window_now(id);
        }
    }

    /// A fake whose windows end up where they were not asked to be
    impl Harness for Offset {
        type B = FakeBackend;
        fn new() -> Self {
            let fake = FakeBackend::with_one_monitor();
            fake.set_frame_offset(FrameOffset {
                left: -7,
                top: 0,
                right: 14,
                bottom: 7,
            });
            Offset(fake)
        }
        fn backend(&self) -> &FakeBackend {
            &self.0
        }
        fn open_window(&self, seed: u32) -> WindowId {
            self.0.open_window(WindowId::from_u32(seed));
            WindowId::from_u32(seed)
        }
        fn destroy_window(&self, id: WindowId) {
            self.0.close_window_now(id);
        }
    }

    /// A fake with none of the optional capabilities, as macOS has
    impl Harness for Bare {
        type B = FakeBackend;
        fn new() -> Self {
            let fake = FakeBackend::with_one_monitor();
            for capability in Capability::ALL {
                fake.set_supported(capability, false);
            }
            Bare(fake)
        }
        fn backend(&self) -> &FakeBackend {
            &self.0
        }
        fn open_window(&self, seed: u32) -> WindowId {
            self.0.open_window(WindowId::from_u32(seed));
            WindowId::from_u32(seed)
        }
        fn destroy_window(&self, id: WindowId) {
            self.0.close_window_now(id);
        }
    }

    #[test]
    fn the_fake_passes_the_contract() {
        check_all::<Plain>();
    }

    #[test]
    fn the_fake_with_a_frame_offset_passes_the_contract() {
        check_all::<Offset>();
    }

    #[test]
    fn the_fake_without_capabilities_passes_the_contract() {
        check_all::<Bare>();
    }

    #[test]
    fn there_are_as_many_checks_as_names() {
        let names: Vec<_> = checks::<Plain>()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(names.len(), unique.len());
        assert_eq!(names.len(), 14);
    }

    /// A backend that breaks the contract is caught: this one forgets that windows die
    struct Forgetful(FakeBackend);

    impl Backend for Forgetful {
        fn windows(&self) -> Vec<crate::WindowInfo> {
            self.0.windows()
        }
        fn window(&self, id: WindowId) -> Option<crate::WindowInfo> {
            self.0.window(id)
        }
        fn foreground_window(&self) -> Option<WindowId> {
            self.0.foreground_window()
        }
        fn monitors(&self) -> crate::Result<Vec<crate::MonitorInfo>> {
            self.0.monitors()
        }
        fn cursor_pos(&self) -> Option<(i32, i32)> {
            self.0.cursor_pos()
        }
        fn set_rect(&self, _: WindowId, _: &Rect) -> crate::Result<()> {
            Ok(())
        }
        fn focus(&self, id: WindowId, mouse: bool) -> crate::Result<()> {
            self.0.focus(id, mouse)
        }
        fn raise(&self, id: WindowId) -> crate::Result<()> {
            self.0.raise(id)
        }
        fn hide(&self, id: WindowId) -> crate::Result<()> {
            self.0.hide(id)
        }
        fn restore(&self, id: WindowId) -> crate::Result<()> {
            self.0.restore(id)
        }
        fn minimize(&self, id: WindowId) -> crate::Result<()> {
            self.0.minimize(id)
        }
        fn unminimize(&self, id: WindowId) -> crate::Result<()> {
            self.0.unminimize(id)
        }
        fn close(&self, id: WindowId) -> crate::Result<()> {
            self.0.close(id)
        }
        fn center_cursor_in(&self, rect: &Rect) -> crate::Result<()> {
            self.0.center_cursor_in(rect)
        }
    }

    struct ForgetfulHarness(Forgetful);

    impl Harness for ForgetfulHarness {
        type B = Forgetful;
        fn new() -> Self {
            ForgetfulHarness(Forgetful(FakeBackend::with_one_monitor()))
        }
        fn backend(&self) -> &Forgetful {
            &self.0
        }
        fn open_window(&self, seed: u32) -> WindowId {
            (self.0).0.open_window(WindowId::from_u32(seed));
            WindowId::from_u32(seed)
        }
        fn destroy_window(&self, id: WindowId) {
            (self.0).0.close_window_now(id);
        }
    }

    #[test]
    fn a_backend_that_answers_ok_for_a_window_that_is_gone_fails_the_contract() {
        let failures = run::<ForgetfulHarness>();

        assert_eq!(failures.len(), 2, "{failures:#?}");
        assert!(
            failures.iter().all(|f| f.contains("set_rect")),
            "{failures:#?}"
        );
    }
}
