use crate::error::Error;
use crate::error::Result;
use crate::id::MonitorId;
use crate::id::WindowId;
use crate::info::MonitorInfo;
use crate::info::WindowInfo;
use komorebi_layouts::Rect;

/// An operation that only some platforms have, and so is asked about with
/// [`Backend::supports`] instead of being required of every backend.
///
/// Calling one of these operations on a backend that does not support it returns
/// [`Error::Unsupported`], whatever the arguments.
///
/// | capability | KDE | Windows | macOS |
/// |---|---|---|---|
/// | [`Capability::Workspaces`] | yes: the compositor's virtual desktops | no: komorebi's own workspaces, shown and hidden with `hide` and `restore` | no: same |
/// | [`Capability::Maximize`] | yes | yes | no |
/// | [`Capability::Opacity`] | yes | yes | no |
/// | [`Capability::FocusFollowsMouse`] | yes (a `kwinrc` setting) | yes (a system setting) | no |
///
/// (`docs/dev/seam.md` section 2.1, rows 16 to 19.)
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Capability {
    /// The five workspace methods: [`Backend::ensure_workspaces`], [`Backend::switch_workspace`],
    /// [`Backend::set_window_workspace`], [`Backend::window_workspace`] and
    /// [`Backend::current_workspaces`]. They exist because KDE delegates workspaces to the
    /// compositor; elsewhere the shared core keeps them itself.
    Workspaces,
    /// [`Backend::maximize`].
    Maximize,
    /// [`Backend::set_opacity`].
    Opacity,
    /// [`Backend::set_focus_follows_mouse`].
    FocusFollowsMouse,
}

impl Capability {
    /// Every capability, for a test that goes through all of them.
    pub const ALL: [Capability; 4] = [
        Capability::Workspaces,
        Capability::Maximize,
        Capability::Opacity,
        Capability::FocusFollowsMouse,
    ];
}

/// The desktop, as the window manager sees it: the one seam between shared code and a platform.
///
/// Only operations that at least two platforms have (`docs/dev/seam.md` section 2.1) are here.
/// What only some platforms have is behind [`Backend::supports`]; what is one platform's own
/// business (borders, the look, the wallpaper, drag frames) is not in this trait at all.
///
/// # Who implements it
///
/// KDE (over the compositor), Windows (over `WindowsApi`), macOS (over `MacosApi` and the
/// Accessibility API) and [`crate::fake::FakeBackend`]. Each method's documentation says which of
/// the three real platforms implement it; an operation that is optional says so and is listed in
/// [`Capability`].
///
/// # Threads (TARGET rules: no platform follows them everywhere today)
///
/// - A backend is `Send + Sync`, and **every method may be called from any thread except a
///   platform's main thread**. Windows is stateless (only window creation and message loops are
///   thread-affine, and a backend does neither). KDE's calls come from threads that hold the
///   window manager lock and never from the bridge's async runtime. macOS already calls the
///   platform from the event, command, reaper, settle, reconciler and animator threads.
/// - **No method waits for the main thread of its own platform.** On macOS the work that must
///   run on main (`NSScreen`, border windows) is either `exec_async`, or answered from data the
///   backend already holds, such as a monitor list that the display callback refreshes. A method
///   that waits for anything (Windows `SetWindowPos` waits for the target window unless the
///   backend is asynchronous; KDE `cursor_pos` waits up to 250 ms) has to say so in its
///   documentation and be bounded. The macOS main-queue deadlock of `docs/dev/seam.md`
///   section 3.3 is the reason for the rule: a call made under the window manager lock must not
///   wait for a closure that needs the lock.
/// - A backend never calls back into the window manager while a method runs. Events travel the
///   other way, through the event channel.
///
/// # Reads after writes
///
/// A write (`set_rect`, `minimize`, `hide`, ...) is a **request**: it returns when the request
/// has been made, not when the platform has acted. A read straight after it (`window`,
/// `window_rect`) returns what the platform defines, and shared code may rely only on this:
///
/// - the answer is a value the platform has reported or, if it keeps a cache, the request itself
///   until an event corrects it (KDE does the latter, `K/kwin/state.rs:594-600`);
/// - the answer need not equal the request: a client may refuse a size, and the platform's idea
///   of the frame may differ from the one `set_rect` takes (Windows reports the visible bounds
///   and `set_rect` adds the shadow offset itself, so a request and its read-back agree there, but
///   KDE and macOS report whatever the window ended up with);
/// - to learn the achieved frame, wait for the event, or call `window_rect` where the layout
///   needs the truth (a refused width, the end of a drag).
///
/// [`crate::fake::FakeBackend`] answers a read after a write at once with the request, clamped to
/// the window's minimum size and moved by its frame offset.
///
/// # Errors
///
/// Window commands fail with [`Error::WindowGone`] when the window is unknown or dead, and
/// otherwise only when the platform refuses ([`Error::Platform`]). Optional operations fail with
/// [`Error::Unsupported`]. No method panics because a window died.
///
/// The compiler enforces the part of the contract that it can see: a backend has to be
/// `Send + Sync`.
///
/// ```rust,compile_fail,E0277
/// use komorebi_backend::{Backend, MonitorInfo, Rect, Result, WindowId, WindowInfo};
/// use std::rc::Rc;
///
/// struct NotShareable(Rc<u8>);
///
/// impl Backend for NotShareable {
///     fn windows(&self) -> Vec<WindowInfo> { vec![] }
///     fn window(&self, _: WindowId) -> Option<WindowInfo> { None }
///     fn foreground_window(&self) -> Option<WindowId> { None }
///     fn monitors(&self) -> Result<Vec<MonitorInfo>> { Ok(vec![]) }
///     fn cursor_pos(&self) -> Option<(i32, i32)> { None }
///     fn set_rect(&self, _: WindowId, _: &Rect) -> Result<()> { Ok(()) }
///     fn focus(&self, _: WindowId, _: bool) -> Result<()> { Ok(()) }
///     fn raise(&self, _: WindowId) -> Result<()> { Ok(()) }
///     fn hide(&self, _: WindowId) -> Result<()> { Ok(()) }
///     fn restore(&self, _: WindowId) -> Result<()> { Ok(()) }
///     fn minimize(&self, _: WindowId) -> Result<()> { Ok(()) }
///     fn unminimize(&self, _: WindowId) -> Result<()> { Ok(()) }
///     fn close(&self, _: WindowId) -> Result<()> { Ok(()) }
///     fn center_cursor_in(&self, _: &Rect) -> Result<()> { Ok(()) }
/// }
/// ```
pub trait Backend: Send + Sync + 'static {
    // ---- queries -------------------------------------------------------------------------------

    /// Returns every window that is a candidate for management and alive, ordered by id.
    ///
    /// Implemented by KDE (its table), Windows (`enum_windows`, `load_workspace_information`)
    /// and macOS (CoreGraphics list joined with Accessibility elements; the join is by title
    /// today, see `docs/dev/seam.md` row 10). On Windows and macOS the daemons enumerate at
    /// start-up and learn from events afterwards, so a backend may be slow here: call it rarely.
    fn windows(&self) -> Vec<WindowInfo>;

    /// Returns what is known about the window, or `None` if the backend has never heard of it.
    /// A destroyed window may stay queryable for a while with [`WindowInfo::alive`] false.
    ///
    /// Implemented by KDE (table lookup, never waits), Windows (assembled from several calls) and
    /// macOS (over Accessibility, so it can fail and then answers `None`).
    fn window(&self, id: WindowId) -> Option<WindowInfo>;

    /// Returns the id of the window that has the focus, if any window does.
    ///
    /// Implemented by KDE (table), Windows (`GetForegroundWindow`; a null handle is `None`) and
    /// macOS (Accessibility system-wide, `None` whenever a step fails).
    fn foreground_window(&self) -> Option<WindowId>;

    /// Returns every connected monitor.
    ///
    /// Implemented by KDE (table, logical pixels), Windows (physical pixels) and macOS (points;
    /// today a live `NSScreen` query that is only legal on the main thread, so a macOS backend
    /// has to serve it from a cache that the display callback refreshes: the TARGET rule above).
    ///
    /// # Errors
    ///
    /// When the platform cannot report its monitors, or reports none.
    fn monitors(&self) -> Result<Vec<MonitorInfo>>;

    /// Returns where the pointer is, if the platform says.
    ///
    /// Implemented by KDE (asks the KWin script and waits up to 250 ms, falling back to the last
    /// position seen), Windows (`GetCursorPos`; a failure is `None`) and macOS (`CGEvent`).
    fn cursor_pos(&self) -> Option<(i32, i32)>;

    // ---- commands ------------------------------------------------------------------------------

    /// Moves and resizes the window to `rect`: a position and a size.
    ///
    /// **This is a request.** It returns when the request has been made (KDE queues it, Windows
    /// and macOS issue it), says nothing about the frame the window ends up with, and may fail
    /// only for an unknown or dead window ([`Error::WindowGone`]) or when the platform refuses
    /// the write outright. A client may refuse the size. See the section "Reads after writes" of
    /// [`Backend`] for what [`Backend::window`] and [`Backend::window_rect`] return afterwards.
    ///
    /// Implemented by KDE (`frameGeometry`, queued, optimistic table write), Windows
    /// (`SetWindowPos`, synchronous by default, `rect` is the visible frame and the shadow offset
    /// is added by the implementation) and macOS (two Accessibility writes, or the animator).
    fn set_rect(&self, id: WindowId, rect: &Rect) -> Result<()>;

    /// Gives the window the focus, and, if `mouse_follows_focus`, puts the pointer in its middle.
    ///
    /// Implemented by KDE (activates, then waits for the window to be foreground before moving
    /// the pointer), Windows and macOS (focus, then `center_cursor_in` on the window's frame when
    /// asked; neither platform has this as one call).
    fn focus(&self, id: WindowId, mouse_follows_focus: bool) -> Result<()>;

    /// Raises the window above the others.
    ///
    /// Implemented by KDE and Windows (raise without activating). macOS has no raise without
    /// focus: its implementation focuses the window as well.
    fn raise(&self, id: WindowId) -> Result<()>;

    /// Takes the window out of sight without closing or minimizing it: how the core keeps the
    /// back of a stack, or a workspace that is not showing, out of the way. The window stays
    /// alive. Hiding a hidden window leaves it as it is.
    ///
    /// Implemented by KDE (a park desktop nobody looks at), Windows (cloak by default) and macOS
    /// (a sliver in a display corner). Whether the user can notice (macOS, Windows `Hide` and
    /// `Minimize`) differs, so the core keeps its own table of what it hid
    /// (`docs/dev/seam.md` finding 11).
    fn hide(&self, id: WindowId) -> Result<()>;

    /// Brings a hidden window back. A window that is not hidden is left as it is.
    ///
    /// Implemented by KDE, Windows and macOS.
    fn restore(&self, id: WindowId) -> Result<()>;

    /// Minimizes the window. Implemented by KDE, Windows and macOS.
    fn minimize(&self, id: WindowId) -> Result<()>;

    /// Brings a minimized window back. Implemented by KDE, Windows (`restore_window`) and macOS.
    fn unminimize(&self, id: WindowId) -> Result<()>;

    /// Asks the window to close. A request: the window may stay, and the event that it is gone
    /// is what the core reacts to. Implemented by KDE, Windows (`WM_CLOSE`) and macOS (presses
    /// the close button, `Ok` when there is none).
    fn close(&self, id: WindowId) -> Result<()>;

    /// Puts the pointer in the middle of `rect`.
    ///
    /// Implemented by Windows and macOS. **KDE ignores `rect`**: KWin can only centre on the
    /// active window, so on KDE the call is correct only right after focusing the window whose
    /// frame `rect` is (`docs/dev/seam.md` row 14, finding 10).
    ///
    /// # Errors
    ///
    /// When the platform refuses to move the pointer.
    fn center_cursor_in(&self, rect: &Rect) -> Result<()>;

    // ---- capabilities --------------------------------------------------------------------------

    /// Returns true if and only if the backend implements the operations behind `capability`. The
    /// default supports none; an operation that is not supported returns [`Error::Unsupported`].
    fn supports(&self, _capability: Capability) -> bool {
        false
    }

    /// Has the platform maximize the window to the work area of its monitor, or, with `maximize`
    /// false, give it back the frame it had. Capability [`Capability::Maximize`]: KDE and
    /// Windows. The default is [`Error::Unsupported`].
    fn maximize(&self, _id: WindowId, _maximize: bool) -> Result<()> {
        Err(Error::Unsupported(Capability::Maximize))
    }

    /// Asks the platform to draw the window with this opacity, 255 being not see-through at all.
    /// Capability [`Capability::Opacity`]: KDE and Windows. The default is
    /// [`Error::Unsupported`].
    fn set_opacity(&self, _id: WindowId, _alpha: u8) -> Result<()> {
        Err(Error::Unsupported(Capability::Opacity))
    }

    /// Asks the platform to give the focus to the window under the pointer as it moves (`true`),
    /// or to go back to the policy the user had (`false`). Capability
    /// [`Capability::FocusFollowsMouse`]: KDE and Windows (which also has an implementation of
    /// its own that does not use this). The default is [`Error::Unsupported`].
    fn set_focus_follows_mouse(&self, _enable: bool) -> Result<()> {
        Err(Error::Unsupported(Capability::FocusFollowsMouse))
    }

    /// Makes sure there are at least as many workspaces as `names`, creating the missing ones.
    /// Capability [`Capability::Workspaces`]: KDE only. The default is [`Error::Unsupported`].
    fn ensure_workspaces(&self, _names: &[String]) -> Result<()> {
        Err(Error::Unsupported(Capability::Workspaces))
    }

    /// Shows workspace `idx` on a monitor, leaving the other monitors as they are. Capability
    /// [`Capability::Workspaces`]: KDE only. The default is [`Error::Unsupported`].
    fn switch_workspace(&self, _monitor: MonitorId, _idx: usize) -> Result<()> {
        Err(Error::Unsupported(Capability::Workspaces))
    }

    /// Puts a window on workspace `idx`. Capability [`Capability::Workspaces`]: KDE only. The
    /// default is [`Error::Unsupported`].
    fn set_window_workspace(&self, _id: WindowId, _idx: usize) -> Result<()> {
        Err(Error::Unsupported(Capability::Workspaces))
    }

    /// Returns the workspace a window is on, `None` for a window that is on none or on every
    /// workspace. Capability [`Capability::Workspaces`]: KDE only. The default is
    /// [`Error::Unsupported`].
    fn window_workspace(&self, _id: WindowId) -> Result<Option<usize>> {
        Err(Error::Unsupported(Capability::Workspaces))
    }

    /// Returns the workspace each monitor shows. Capability [`Capability::Workspaces`]: KDE only.
    /// The default is [`Error::Unsupported`].
    fn current_workspaces(&self) -> Result<Vec<(MonitorId, usize)>> {
        Err(Error::Unsupported(Capability::Workspaces))
    }

    // ---- batches -------------------------------------------------------------------------------

    /// Starts a batch: from here until the matching [`Backend::end_batch`], what the writes
    /// change on screen should appear together. Batches nest, and a backend counts them. Use
    /// [`Batch`] instead of calling this and `end_batch` by hand.
    ///
    /// The default does nothing. macOS implements it with SkyLight (`SLSDisableUpdate`,
    /// `BatchedUpdates::begin`, which the core-adjacent code already brackets groups of window
    /// operations with); KDE batches by construction (the answer to a held poll carries every
    /// queued command) and Windows does not batch.
    fn begin_batch(&self) {}

    /// Ends the batch started by the matching [`Backend::begin_batch`]. The default does
    /// nothing.
    fn end_batch(&self) {}

    // ---- provided ------------------------------------------------------------------------------

    /// Returns true if and only if the backend has a live window with this id.
    fn is_window(&self, id: WindowId) -> bool {
        self.window(id).is_some_and(|w| w.alive)
    }

    /// Returns the window's frame, if the backend knows the window: the `rect` of
    /// [`Backend::window`], in the platform's own definition (Windows: the visible bounds
    /// without the shadow; KDE: the frame geometry; macOS: the Accessibility position and size).
    fn window_rect(&self, id: WindowId) -> Option<Rect> {
        self.window(id).map(|w| w.rect)
    }

    /// Returns the id of the monitor whose bounds contain the point. A point outside every
    /// monitor gives `None` on KDE and macOS, and the **nearest** monitor on Windows
    /// (`MONITOR_DEFAULTTONEAREST`), so callers must not use `None` to detect "off screen".
    /// The default implements the KDE and macOS answer.
    fn monitor_from_point(&self, x: i32, y: i32) -> Option<MonitorId> {
        self.monitors().ok()?.into_iter().find_map(|m| {
            let (l, t) = (m.size.left, m.size.top);
            let (r, b) = (l + m.size.right, t + m.size.bottom);
            (x >= l && x < r && y >= t && y < b).then_some(m.id)
        })
    }
}

/// A batch scope: writes made while it lives should appear together, and the batch ends when it
/// is dropped, also when the code in between panics.
///
/// A backend that cannot batch ignores it ([`Backend::begin_batch`] does nothing by default), so
/// shared code may bracket every layout with it.
///
/// # Example
///
/// ```rust
/// use komorebi_backend::{Backend, Batch, Rect, WindowId};
/// use komorebi_backend::fake::FakeBackend;
///
/// let fake = FakeBackend::with_one_monitor();
/// fake.open_window(WindowId(1));
/// {
///     let _batch = Batch::begin(&fake);
///     fake.set_rect(WindowId(1), &Rect { left: 0, top: 0, right: 100, bottom: 100 })?;
///     assert_eq!(fake.open_batches(), 1);
/// }
/// assert_eq!(fake.open_batches(), 0);
/// # Ok::<(), komorebi_backend::Error>(())
/// ```
#[must_use = "the batch ends when this is dropped"]
pub struct Batch<'a, B: Backend + ?Sized>(&'a B);

impl<'a, B: Backend + ?Sized> Batch<'a, B> {
    /// Starts a batch on `backend`.
    pub fn begin(backend: &'a B) -> Self {
        backend.begin_batch();
        Self(backend)
    }
}

impl<B: Backend + ?Sized> Drop for Batch<'_, B> {
    fn drop(&mut self) {
        self.0.end_batch();
    }
}
