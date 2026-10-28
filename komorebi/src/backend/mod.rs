//! Everything the window manager needs from the desktop, behind one trait.
//!
//! The window manager logic (windows, containers, workspaces, monitors) never talks to KWin, to
//! `/proc` or to D-Bus. It talks to a [`Backend`]. A real backend asks the compositor; the one the
//! tests use is [`fake::FakeBackend`], an in-memory desktop that does what it is told and records
//! what it was asked.
//!
//! # The backend is process-wide
//!
//! The backend is a singleton, like the other globals of this crate, so that a window can be a
//! small `Copy` handle that carries only its id: `window.set_position(..)` needs no context passed
//! to it. [`set_backend`] installs it once at startup, and [`backend`] returns it. A test that needs
//! a private desktop uses [`with_backend`], which overrides the backend for the current thread
//! only and puts the previous one back afterwards, so tests cannot leak a desktop into each other.
//!
//! # Queries and commands
//!
//! Queries are answered from the backend's own table, which compositor events keep up to date, so
//! they never wait for the compositor. A window that has just been closed stays queryable for a
//! short time, with [`WindowInfo::alive`] false, so that the event that mentions it can still be
//! logged. Commands return once the compositor has accepted them, not once the client has acted on
//! them, and an implementation may batch them.
//!
//! # Workspaces
//!
//! komorebi's workspaces are the compositor's virtual desktops, per monitor: the window manager
//! asks for workspace `i` to be shown on a monitor ([`Backend::switch_workspace`]) and for a window
//! to be put on workspace `i` ([`Backend::set_window_workspace`]), and the compositor does the
//! showing, the hiding and the focusing. When something else changes what a monitor shows, the
//! backend raises an event and the window manager follows it.
//!
//! # Hiding
//!
//! The one thing the window manager hides itself is the back of a stack: a container tiles one
//! window, so the others in it are taken out of sight ([`Backend::hide`]) and brought back
//! ([`Backend::restore`]) when they are given the tile. A hidden window stays alive, stays on its
//! workspace and is not minimized, so what the compositor reports about it is not taken for the
//! user's doing (the window manager keeps a table of the windows it has hidden).
//!
//! # The look
//!
//! How windows look is asked of the compositor and drawn by it, since only the compositor has the
//! frames, the corners and the shadows. The window manager describes a [`Look`] and the backend
//! sees that it is drawn ([`Backend::apply_look`]); the window manager is told whether the
//! compositor draws a border, because a border that is drawn takes room around each window, which
//! the layout leaves. The color of each window's border is another request
//! ([`Backend::set_border_colours`]), which a compositor may not be able to honor.

#![deny(missing_docs)]

use crate::core::Rect;
use color_eyre::eyre;
use parking_lot::RwLock;
use std::cell::RefCell;
use std::sync::Arc;

pub mod fake;

/// What the compositor currently knows about one window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowInfo {
    /// The compositor's id for the window, which is how every other part of komorebi names it.
    pub id: u32,
    /// The id of the process that owns the window.
    pub pid: i32,
    /// The window's title.
    pub title: String,
    /// The Wayland `app_id` (the X11 `WM_CLASS` for a client that runs on XWayland).
    pub class: String,
    /// The file name of the executable, read from `/proc/<pid>/exe`.
    pub exe: String,
    /// The full path of the executable.
    pub path: String,
    /// The window's frame.
    pub rect: Rect,
    /// Whether the window is minimized.
    pub minimized: bool,
    /// Whether the compositor considers this a normal application window, as opposed to a dialog,
    /// a splash screen, a utility window, a dock and so on.
    pub normal: bool,
    /// False once the window has been destroyed.
    pub alive: bool,
}

/// A connected monitor, in logical pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonitorInfo {
    /// The compositor's id for the monitor.
    pub id: u32,
    /// The name of the output (`DP-1`, `Virtual-0`).
    pub name: String,
    /// An identity for the monitor that stays the same across a hot-plug and a restart.
    pub serial_number_id: String,
    /// The monitor's bounds.
    pub size: Rect,
    /// `size` minus the panels and other areas the compositor reserves.
    pub work_area: Rect,
}

/// How the compositor is asked to dress the windows komorebi arranges.
///
/// komorebi decides what the windows look like and the compositor draws it, so this only
/// describes. The default asks for nothing.
///
/// # Example
///
/// ```rust
/// use komorebi::backend::AnimationLook;
/// use komorebi::backend::BorderLook;
/// use komorebi::backend::Look;
///
/// assert_eq!(Look::default().border, None);
/// assert_eq!(Look::default().animation, None);
///
/// let look = Look {
///     border: Some(BorderLook {
///         width: 10,
///         radius: 12,
///         focused: (66, 165, 245),
///         unfocused: (128, 128, 128),
///     }),
///     animation: Some(AnimationLook {
///         duration_ms: 250,
///         curve: "InOutSine",
///     }),
/// };
/// assert!(look.border.is_some() && look.animation.is_some());
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    /// A colored outline outside every window, or none.
    pub border: Option<BorderLook>,
    /// Animate windows going to their new places, or not.
    pub animation: Option<AnimationLook>,
}

/// How windows are animated going to their places.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationLook {
    /// How long the animation of a window takes, in milliseconds.
    pub duration_ms: u64,
    /// The Qt easing curve of the animation, `QEasingCurve.<this>`.
    pub curve: &'static str,
}

/// The outline around every window.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderLook {
    /// The thickness of the outline, outside the window, in pixels.
    pub width: i32,
    /// The radius of the window's corners, in pixels.
    pub radius: i32,
    /// The outline of the focused window, as red, green and blue.
    pub focused: (u8, u8, u8),
    /// The outline of every other window.
    pub unfocused: (u8, u8, u8),
}

/// The desktop, as the window manager sees it.
///
/// This is the **only** way the window manager reaches the compositor. Everything else in the crate
/// is testable without one.
///
/// # Who implements it, who calls it
///
/// A backend is implemented once per compositor, and by the in-memory [`fake::FakeBackend`]. It is
/// called by the window manager, through [`backend`]; nothing else should hold a reference to it.
///
/// # Contract
///
/// - Queries never block on the compositor and never fail, except [`Backend::monitors`], which may
///   fail when the compositor cannot say.
/// - A command about a window that does not exist, or is no longer alive, returns an error. It does
///   not panic.
/// - A backend is `Send + Sync`: the window manager calls it from several threads.
/// - Workspace `i` is the compositor's virtual desktop `i` on every monitor, and each monitor shows
///   one of them at a time ([`Backend::switch_workspace`]). The compositor shows the windows of the
///   workspace in front and hides the others; the window manager never hides a window to leave a
///   workspace.
/// - A hidden window ([`Backend::hide`]) is still alive and still on its workspace: it is out of
///   sight until [`Backend::restore`], and [`Backend::window_workspace`] names the workspace it
///   comes back to.
/// - A backend asked to show a workspace, or to put a window on one, that has no desktop yet makes
///   the desktops up to it first.
/// - The look is a request: a backend that cannot draw what it is asked for says so in the answer
///   of [`Backend::apply_look`] and the window manager goes on without it.
///
/// The compiler enforces the first and last parts of the contract that it can see. A backend that
/// leaves out a method does not compile:
///
/// ```rust,compile_fail,E0046
/// use komorebi::backend::Backend;
///
/// struct Nothing;
///
/// impl Backend for Nothing {}
/// ```
///
/// And neither does one that cannot be shared between threads:
///
/// ```rust,compile_fail,E0277
/// # use std::rc::Rc;
/// # use color_eyre::eyre;
/// # use komorebi::backend::{Backend, MonitorInfo, WindowInfo};
/// # use komorebi::core::Rect;
/// struct NotShareable(Rc<u8>);
///
/// # impl Backend for NotShareable {
/// #     fn windows(&self) -> Vec<WindowInfo> { vec![] }
/// #     fn window(&self, _: u32) -> Option<WindowInfo> { None }
/// #     fn foreground_window(&self) -> Option<u32> { None }
/// #     fn monitors(&self) -> eyre::Result<Vec<MonitorInfo>> { Ok(vec![]) }
/// #     fn cursor_pos(&self) -> Option<(i32, i32)> { None }
/// #     fn set_rect(&self, _: u32, _: &Rect) -> eyre::Result<()> { Ok(()) }
/// #     fn focus(&self, _: u32, _: bool) -> eyre::Result<()> { Ok(()) }
/// #     fn raise(&self, _: u32) -> eyre::Result<()> { Ok(()) }
/// #     fn hide(&self, _: u32) -> eyre::Result<()> { Ok(()) }
/// #     fn restore(&self, _: u32) -> eyre::Result<()> { Ok(()) }
/// #     fn minimize(&self, _: u32) -> eyre::Result<()> { Ok(()) }
/// #     fn unminimize(&self, _: u32) -> eyre::Result<()> { Ok(()) }
/// #     fn close(&self, _: u32) -> eyre::Result<()> { Ok(()) }
/// #     fn center_cursor_in(&self, _: &Rect) -> eyre::Result<()> { Ok(()) }
/// #     fn ensure_workspaces(&self, _: &[String]) -> eyre::Result<()> { Ok(()) }
/// #     fn switch_workspace(&self, _: u32, _: usize) -> eyre::Result<()> { Ok(()) }
/// #     fn set_window_workspace(&self, _: u32, _: usize) -> eyre::Result<()> { Ok(()) }
/// #     fn window_workspace(&self, _: u32) -> Option<usize> { None }
/// #     fn current_workspaces(&self) -> Vec<(u32, usize)> { vec![] }
/// # }
/// ```
pub trait Backend: Send + Sync + 'static {
    // ---- queries -------------------------------------------------------------------------------

    /// Returns every window on the desktop that is a candidate for management, ordered by id.
    fn windows(&self) -> Vec<WindowInfo>;

    /// Returns what is known about the window, or `None` if the compositor has never heard of it.
    fn window(&self, id: u32) -> Option<WindowInfo>;

    /// Returns the id of the window that has the focus, if any window does.
    fn foreground_window(&self) -> Option<u32>;

    /// Returns every connected monitor.
    ///
    /// # Errors
    ///
    /// When the compositor cannot report its outputs.
    fn monitors(&self) -> eyre::Result<Vec<MonitorInfo>>;

    /// Returns where the pointer is, if the compositor says.
    fn cursor_pos(&self) -> Option<(i32, i32)>;

    // ---- commands ------------------------------------------------------------------------------

    /// Moves and resizes the window to `rect`, a position and a size.
    ///
    /// A client may refuse the size (a minimum size, for one), so the frame the window ends up with
    /// is [`Backend::window_rect`], not `rect`.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive.
    fn set_rect(&self, id: u32, rect: &Rect) -> eyre::Result<()>;

    /// Gives the window the focus, and, if `mouse_follows_focus`, puts the pointer in its middle.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive.
    fn focus(&self, id: u32, mouse_follows_focus: bool) -> eyre::Result<()>;

    /// Raises the window above the others.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive.
    fn raise(&self, id: u32) -> eyre::Result<()>;

    /// Takes the window out of sight without closing or minimizing it, which is how the window
    /// manager keeps the back of a stack out of the way. On KWin it goes to a virtual desktop that
    /// nobody looks at.
    ///
    /// The window stays on its workspace ([`Backend::window_workspace`]), and
    /// [`Backend::set_window_workspace`] changes the workspace it comes back to, not where it is
    /// hidden. A window that is hidden already is left as it is.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive, or the compositor cannot make the
    /// place the window is hidden in.
    fn hide(&self, id: u32) -> eyre::Result<()>;

    /// Brings a hidden window back, to the workspace it is on. A window that is not hidden is left
    /// as it is.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive.
    fn restore(&self, id: u32) -> eyre::Result<()>;

    /// Minimizes the window.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive.
    fn minimize(&self, id: u32) -> eyre::Result<()>;

    /// Brings a minimized window back.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive.
    fn unminimize(&self, id: u32) -> eyre::Result<()>;

    /// Asks the window to close.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive.
    fn close(&self, id: u32) -> eyre::Result<()>;

    /// Puts the pointer in the middle of `rect`.
    ///
    /// # Errors
    ///
    /// When the compositor refuses to move the pointer.
    fn center_cursor_in(&self, rect: &Rect) -> eyre::Result<()>;

    // ---- workspaces ----------------------------------------------------------------------------
    //
    // komorebi's workspaces are the compositor's own virtual desktops: workspace `i` is desktop `i`
    // on every monitor, and each monitor shows one at a time. Showing, hiding and focusing the
    // windows of a workspace is then the compositor's business; komorebi tiles what is in front and
    // follows the compositor when something else (a pager, a shortcut, an app raising a window)
    // changes what a monitor shows.

    /// Makes sure there are at least as many desktops as `names`, creating the missing ones with
    /// those names.
    ///
    /// A desktop that is already there keeps its name, unless nobody chose it (the compositor's
    /// own "Desktop 2"), in which case it may take the name it is given.
    ///
    /// # Errors
    ///
    /// When the compositor refuses to create or rename a desktop.
    fn ensure_workspaces(&self, names: &[String]) -> eyre::Result<()>;

    /// Shows workspace `idx` on the monitor `monitor_id`, leaving the other monitors as they are.
    ///
    /// The compositor shows the windows on that workspace, hides the rest, and gives the focus to a
    /// window of that workspace by itself.
    ///
    /// # Errors
    ///
    /// When there is no desktop for `idx` and one cannot be made.
    fn switch_workspace(&self, monitor_id: u32, idx: usize) -> eyre::Result<()>;

    /// Puts a window on workspace `idx`. It is visible at once to [`Backend::window_workspace`].
    ///
    /// # Errors
    ///
    /// When the window does not exist, or there is no desktop for `idx` and one cannot be made.
    fn set_window_workspace(&self, id: u32, idx: usize) -> eyre::Result<()>;

    /// Returns the workspace a window is on, or `None` for a window the compositor does not know or
    /// that is on every workspace.
    fn window_workspace(&self, id: u32) -> Option<usize>;

    /// Returns the workspace each monitor shows, as `(monitor id, workspace index)`.
    fn current_workspaces(&self) -> Vec<(u32, usize)>;

    // ---- look, which a backend that draws nothing need not say anything about --------------------

    /// Asks the compositor to draw this look, and returns true if and only if window borders are
    /// being drawn afterwards. An animation is asked for the same way, and whether it is drawn is
    /// not answered.
    ///
    /// A drawn border takes room around each window, which the layout has to leave, so the answer
    /// is what the window manager reads to know that. A backend remembers the look it applied last
    /// and does nothing when it is asked for the same again, so the window manager may call this as
    /// often as it likes. The default draws nothing and answers false.
    ///
    /// # Errors
    ///
    /// When the compositor cannot be reached.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi::backend::Backend;
    /// use komorebi::backend::BorderLook;
    /// use komorebi::backend::Look;
    /// use komorebi::backend::fake::FakeBackend;
    ///
    /// let fake = FakeBackend::with_one_monitor();
    /// fake.set_borders_drawn(true);
    /// let look = Look {
    ///     border: Some(BorderLook {
    ///         width: 10,
    ///         radius: 10,
    ///         focused: (66, 165, 245),
    ///         unfocused: (128, 128, 128),
    ///     }),
    ///     animation: None,
    /// };
    ///
    /// assert!(fake.apply_look(&look)?);
    /// assert!(!fake.apply_look(&Look::default())?);
    /// # Ok::<(), color_eyre::eyre::Report>(())
    /// ```
    fn apply_look(&self, _look: &Look) -> eyre::Result<bool> {
        Ok(false)
    }

    /// Tells the compositor the color of each window's border, as `(window id, 0xRRGGBB)`: the
    /// window manager picks it by what the window is to the layout (the focused window of the
    /// focused monitor, a stack, a monocle, a floating window, a locked container). A window that
    /// is not named has the border [`Backend::apply_look`] gave it. The colors replace the ones
    /// given before, and a backend sends the compositor nothing it sent last time.
    ///
    /// Returns true if and only if the compositor can give each window a color of its own. When it
    /// cannot, the borders are the look's two colors, one for the focused window and one for every
    /// other. The default draws nothing and answers false.
    ///
    /// # Errors
    ///
    /// When the compositor cannot be reached.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi::backend::Backend;
    /// use komorebi::backend::fake::FakeBackend;
    ///
    /// let fake = FakeBackend::with_one_monitor();
    /// fake.open_window(7, "foot", "foot", "a terminal");
    ///
    /// assert!(fake.set_border_colours(&[(7, 0x42A5F5)])?);
    /// assert_eq!(fake.border_colour_of(7), Some(0x42A5F5));
    /// # Ok::<(), color_eyre::eyre::Report>(())
    /// ```
    fn set_border_colours(&self, _colours: &[(u32, u32)]) -> eyre::Result<bool> {
        Ok(false)
    }

    /// Asks the compositor to draw the window with this opacity: 255 is not see-through at all.
    /// The default does nothing.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi::backend::Backend;
    /// use komorebi::backend::fake::FakeBackend;
    ///
    /// let fake = FakeBackend::with_one_monitor();
    /// fake.open_window(7, "foot", "foot", "a terminal");
    /// assert_eq!(fake.opacity_of(7), 255);
    ///
    /// fake.set_opacity(7, 150)?;
    /// assert_eq!(fake.opacity_of(7), 150);
    /// # Ok::<(), color_eyre::eyre::Report>(())
    /// ```
    fn set_opacity(&self, _id: u32, _alpha: u8) -> eyre::Result<()> {
        Ok(())
    }

    /// Asks the compositor to give the focus to the window under the pointer, as the pointer moves
    /// (`true`), or to stop doing that and go back to the focus policy the user had (`false`). The
    /// default does nothing.
    ///
    /// # Errors
    ///
    /// When the compositor's configuration cannot be changed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi::backend::Backend;
    /// use komorebi::backend::fake::Command;
    /// use komorebi::backend::fake::FakeBackend;
    ///
    /// let fake = FakeBackend::with_one_monitor();
    /// fake.set_focus_follows_mouse(true)?;
    /// assert_eq!(fake.commands(), [Command::FocusFollowsMouse(true)]);
    /// # Ok::<(), color_eyre::eyre::Report>(())
    /// ```
    fn set_focus_follows_mouse(&self, _enable: bool) -> eyre::Result<()> {
        Ok(())
    }

    // ---- provided ------------------------------------------------------------------------------

    /// Called once as komorebi exits, after the window manager has put back what it moved, so that
    /// the backend can give up whatever it created itself.
    ///
    /// The default does nothing. Implementors should not need to override it unless they created
    /// something in the compositor.
    fn shutdown(&self) {}

    /// Has the compositor maximize the window to the work area of its monitor, panels left alone,
    /// or, with `maximize` false, give it back the frame it had before.
    ///
    /// The default does nothing, for a compositor that has no maximize of its own.
    ///
    /// # Errors
    ///
    /// When the window does not exist or is no longer alive.
    fn maximize(&self, _id: u32, _maximize: bool) -> eyre::Result<()> {
        Ok(())
    }

    /// Returns true if and only if the compositor has a live window with this id.
    fn is_window(&self, id: u32) -> bool {
        self.window(id).is_some_and(|w| w.alive)
    }

    /// Returns the window's frame, if the compositor knows the window.
    fn window_rect(&self, id: u32) -> Option<Rect> {
        self.window(id).map(|w| w.rect)
    }

    /// Returns the id of the monitor whose bounds contain the point.
    fn monitor_from_point(&self, x: i32, y: i32) -> Option<u32> {
        self.monitors().ok()?.into_iter().find_map(|m| {
            let (l, t) = (m.size.left, m.size.top);
            let (r, b) = (l + m.size.right, t + m.size.bottom);
            (x >= l && x < r && y >= t && y < b).then_some(m.id)
        })
    }
}

static BACKEND: RwLock<Option<Arc<dyn Backend>>> = RwLock::new(None);

thread_local! {
    static OVERRIDE: RefCell<Option<Arc<dyn Backend>>> = const { RefCell::new(None) };
}

/// Installs the process-wide backend. Called once at startup.
pub fn set_backend(backend: Arc<dyn Backend>) {
    *BACKEND.write() = Some(backend);
}

/// Returns the backend for this thread: the thread's override if there is one, else the
/// process-wide one.
///
/// # Panics
///
/// If no backend has been installed. That is a bug in the order of start-up, not a condition at
/// run time. Install one with [`set_backend`], or with [`with_backend`] in a test.
pub fn backend() -> Arc<dyn Backend> {
    OVERRIDE
        .with(|o| o.borrow().clone())
        .or_else(|| BACKEND.read().clone())
        .or_else(test_default)
        .expect(
            "no backend installed: call backend::set_backend (or backend::with_backend in tests)",
        )
}

/// The crate's own unit tests get a private, permissive fake per thread when they install nothing,
/// so that a test of pure window manager logic needs no set-up. Everything else gets no default.
#[cfg(test)]
fn test_default() -> Option<Arc<dyn Backend>> {
    OVERRIDE.with(|o| {
        let backend: Arc<dyn Backend> = Arc::new(fake::FakeBackend::permissive());
        *o.borrow_mut() = Some(backend.clone());
        Some(backend)
    })
}

#[cfg(not(test))]
fn test_default() -> Option<Arc<dyn Backend>> {
    None
}

/// Runs `f` with `backend` as this thread's backend, and puts the previous override back
/// afterwards, also when `f` panics.
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
///
/// use komorebi::backend::Backend;
/// use komorebi::backend::backend;
/// use komorebi::backend::fake::FakeBackend;
/// use komorebi::backend::with_backend;
///
/// let fake = Arc::new(FakeBackend::with_one_monitor());
/// fake.open_window(7, "foot", "foot", "a terminal");
///
/// let titles = with_backend(fake.clone(), || {
///     backend().windows().into_iter().map(|w| w.title).collect::<Vec<_>>()
/// });
///
/// assert_eq!(titles, ["a terminal"]);
/// ```
pub fn with_backend<R>(backend: Arc<dyn Backend>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Arc<dyn Backend>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|o| *o.borrow_mut() = self.0.take());
        }
    }

    let _restore = Restore(OVERRIDE.with(|o| o.borrow_mut().replace(backend)));
    f()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake::FakeBackend;

    #[test]
    fn a_thread_that_installs_nothing_gets_a_permissive_fake_in_unit_tests() {
        let id = 41;

        assert!(backend().window(id).is_some());
    }

    #[test]
    fn with_backend_overrides_the_backend_and_puts_the_previous_one_back() {
        let outer = Arc::new(FakeBackend::with_one_monitor());
        outer.open_window(1, "outer", "outer", "outer");
        let inner = Arc::new(FakeBackend::with_one_monitor());
        inner.open_window(2, "inner", "inner", "inner");

        with_backend(outer, || {
            with_backend(inner, || {
                assert!(backend().is_window(2));
                assert!(!backend().is_window(1));
            });

            assert!(backend().is_window(1));
            assert!(!backend().is_window(2));
        });
    }

    #[test]
    fn with_backend_puts_the_previous_backend_back_when_the_closure_panics() {
        let outer = Arc::new(FakeBackend::with_one_monitor());
        outer.open_window(1, "outer", "outer", "outer");

        with_backend(outer, || {
            let inner = Arc::new(FakeBackend::with_one_monitor());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with_backend(inner, || panic!("a test"))
            }));

            assert!(result.is_err());
            assert!(backend().is_window(1));
        });
    }

    #[test]
    fn the_provided_methods_are_answered_from_the_queries() {
        let fake = FakeBackend::with_one_monitor();
        fake.open_window(5, "foot", "foot", "a terminal");

        assert!(fake.is_window(5));
        assert!(!fake.is_window(6));
        assert_eq!(fake.window_rect(5), fake.rect_of(5));
        assert_eq!(fake.monitor_from_point(10, 10), Some(1));
        assert_eq!(fake.monitor_from_point(5000, 10), None);
    }

    #[test]
    fn a_destroyed_window_is_no_longer_a_window_but_can_still_be_asked_about() {
        let fake = FakeBackend::with_one_monitor();
        fake.open_window(5, "foot", "foot", "a terminal");

        fake.close_window_now(5);

        assert!(!fake.is_window(5));
        assert!(fake.window(5).is_some_and(|w| !w.alive));
        assert!(fake.windows().is_empty());
    }
}
