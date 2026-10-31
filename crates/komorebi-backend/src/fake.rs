//! An in-memory desktop, so that window manager logic can be tested without a platform.
//!
//! A [`FakeBackend`] does what it is told, at once and exactly: a window asked to move to a
//! rectangle is at that rectangle, unless it was given a minimum size or the fake was given a
//! frame offset (see below). Every command is recorded, so a test can assert on what the window
//! manager asked for, and a test can do what a user does (move a window, close it, change which
//! one has the focus) without the window manager having asked.
//!
//! The fake implements the whole of [`Backend`], every [`Capability`] included (switch one off
//! with [`FakeBackend::set_supported`] to test code that must cope without it).
//!
//! # Failure injection
//!
//! [`FakeBackend::fail_next`] makes the next call of an operation fail with the error you give
//! it, as an Accessibility write that is refused or a compositor that has gone away does, and
//! [`FakeBackend::fail_next_for`] does the same for the next call about one window. The call is
//! recorded as a command (it was asked) and changes nothing else.
//!
//! # Frame offsets
//!
//! Windows reports a window's visible frame and adds the invisible shadow border itself when it
//! is asked to move one, and an application may not draw exactly where it was told. A
//! [`FrameOffset`] makes the fake behave like that: whatever the window is asked to be, it ends
//! up that much bigger and elsewhere, and [`Backend::window_rect`] says so.
//!
//! # Example
//!
//! ```rust
//! use komorebi_backend::fake::{Command, FakeBackend, Op};
//! use komorebi_backend::{Backend, Error, Rect, WindowId};
//!
//! let fake = FakeBackend::with_one_monitor();
//! let id = WindowId(1);
//! fake.open_window(id);
//!
//! let tile = Rect { left: 0, top: 0, right: 960, bottom: 1050 };
//! fake.set_rect(id, &tile)?;
//! assert_eq!(fake.rect_of(id), Some(tile));
//! assert_eq!(fake.commands(), [Command::SetRect(id, tile)]);
//!
//! fake.fail_next(Op::Focus, Error::platform("refused"));
//! assert!(fake.focus(id, false).is_err());
//! assert!(fake.focus(id, false).is_ok());
//! # Ok::<(), Error>(())
//! ```

use crate::Backend;
use crate::Capability;
use crate::Error;
use crate::MonitorId;
use crate::MonitorInfo;
use crate::Result;
use crate::WindowId;
use crate::WindowInfo;
use komorebi_layouts::Rect;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

/// The command type of a fake with no platform-specific commands of its own: nothing is ever
/// logged as one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoExt {}

/// A command the window manager sent to the backend. `X` is the type of the commands that a
/// platform's own extension of the fake logs ([`Command::Ext`]); the shared fake has none.
#[derive(Clone, Debug, PartialEq)]
pub enum Command<X = NoExt> {
    /// [`Backend::set_rect`]: the window and the rectangle it was asked to take.
    SetRect(WindowId, Rect),
    /// [`Backend::focus`]: the window, and whether the pointer was to follow.
    Focus(WindowId, bool),
    /// [`Backend::raise`].
    Raise(WindowId),
    /// [`Backend::hide`].
    Hide(WindowId),
    /// [`Backend::restore`].
    Restore(WindowId),
    /// [`Backend::minimize`].
    Minimize(WindowId),
    /// [`Backend::unminimize`].
    Unminimize(WindowId),
    /// [`Backend::close`].
    Close(WindowId),
    /// [`Backend::center_cursor_in`].
    CenterCursor(Rect),
    /// [`Backend::ensure_workspaces`]: how many workspaces were asked for.
    EnsureWorkspaces(usize),
    /// [`Backend::maximize`]: the window, and whether it was to be maximized or restored.
    Maximize(WindowId, bool),
    /// [`Backend::switch_workspace`]: the monitor, and the workspace it was asked to show.
    SwitchWorkspace(MonitorId, usize),
    /// [`Backend::set_window_workspace`]: the window, and the workspace it was put on.
    SetWindowWorkspace(WindowId, usize),
    /// [`Backend::set_opacity`]: the window, and how opaque it was asked to be.
    SetOpacity(WindowId, u8),
    /// [`Backend::set_focus_follows_mouse`]: whether it was turned on or off.
    FocusFollowsMouse(bool),
    /// [`Backend::begin_batch`].
    BatchBegin,
    /// [`Backend::end_batch`].
    BatchEnd,
    /// A command of the platform's own extension of the fake, logged by
    /// [`FakeBackend::record_ext`] so that it keeps its place among the others.
    Ext(X),
}

/// The operations whose failure a test can inject: every one that returns a [`Result`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Op {
    /// [`Backend::monitors`].
    Monitors,
    /// [`Backend::set_rect`].
    SetRect,
    /// [`Backend::focus`].
    Focus,
    /// [`Backend::raise`].
    Raise,
    /// [`Backend::hide`].
    Hide,
    /// [`Backend::restore`].
    Restore,
    /// [`Backend::minimize`].
    Minimize,
    /// [`Backend::unminimize`].
    Unminimize,
    /// [`Backend::close`].
    Close,
    /// [`Backend::center_cursor_in`].
    CenterCursor,
    /// [`Backend::maximize`].
    Maximize,
    /// [`Backend::set_opacity`].
    SetOpacity,
    /// [`Backend::set_focus_follows_mouse`].
    SetFocusFollowsMouse,
    /// [`Backend::ensure_workspaces`].
    EnsureWorkspaces,
    /// [`Backend::switch_workspace`].
    SwitchWorkspace,
    /// [`Backend::set_window_workspace`].
    SetWindowWorkspace,
    /// [`Backend::window_workspace`].
    WindowWorkspace,
    /// [`Backend::current_workspaces`].
    CurrentWorkspaces,
}

/// How much bigger, and how far over, a window ends up than it was asked to be: the shadow border
/// that Windows adds, or the decoration of a client that draws its own.
///
/// It is added to the requested [`Rect`] field by field (`right` and `bottom` being the width
/// and height, so a positive `right` makes the window wider).
///
/// # Example
///
/// ```rust
/// use komorebi_backend::fake::{FakeBackend, FrameOffset};
/// use komorebi_backend::{Backend, Rect, WindowId};
///
/// let fake = FakeBackend::with_one_monitor();
/// fake.open_window(WindowId(1));
/// fake.set_frame_offset(FrameOffset { left: -7, top: 0, right: 14, bottom: 7 });
///
/// fake.set_rect(WindowId(1), &Rect { left: 100, top: 100, right: 400, bottom: 300 })?;
///
/// assert_eq!(
///     fake.window_rect(WindowId(1)),
///     Some(Rect { left: 93, top: 100, right: 414, bottom: 307 })
/// );
/// # Ok::<(), komorebi_backend::Error>(())
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameOffset {
    /// Added to `left`.
    pub left: i32,
    /// Added to `top`.
    pub top: i32,
    /// Added to `right`, the width.
    pub right: i32,
    /// Added to `bottom`, the height.
    pub bottom: i32,
}

impl FrameOffset {
    fn apply(self, rect: Rect) -> Rect {
        Rect {
            left: rect.left + self.left,
            top: rect.top + self.top,
            right: rect.right + self.right,
            bottom: rect.bottom + self.bottom,
        }
    }
}

struct State<X> {
    /// When set, a command about a window nobody opened creates that window first (see
    /// [`FakeBackend::permissive`]).
    permissive: bool,
    windows: HashMap<WindowId, WindowInfo>,
    /// the windows that are hidden, in the order they were
    hidden: Vec<WindowId>,
    min_sizes: HashMap<WindowId, (i32, i32)>,
    frame_offset: FrameOffset,
    window_frame_offsets: HashMap<WindowId, FrameOffset>,
    monitors: Vec<MonitorInfo>,
    foreground: Option<WindowId>,
    cursor: Option<(i32, i32)>,
    commands: Vec<Command<X>>,
    /// windows that are maximized, with the frame to give back
    maximized: HashMap<WindowId, Rect>,
    /// windows that are fullscreen, with the frame they had before
    fullscreen: HashMap<WindowId, Rect>,
    /// how many workspaces there are
    desktops: usize,
    /// the workspace every window is on
    window_ws: HashMap<WindowId, usize>,
    /// the workspace each monitor shows
    shown: HashMap<MonitorId, usize>,
    /// how opaque each window was last asked to be
    opacity: HashMap<WindowId, u8>,
    supported: HashSet<Capability>,
    /// failures to hand out, oldest first: the operation, the window it is about if the failure
    /// is only for one, and the error
    failures: Vec<(Op, Option<WindowId>, Error)>,
    batches: usize,
}

impl<X> Default for State<X> {
    fn default() -> Self {
        Self {
            permissive: false,
            windows: HashMap::new(),
            hidden: Vec::new(),
            min_sizes: HashMap::new(),
            frame_offset: FrameOffset::default(),
            window_frame_offsets: HashMap::new(),
            monitors: Vec::new(),
            foreground: None,
            cursor: None,
            commands: Vec::new(),
            maximized: HashMap::new(),
            fullscreen: HashMap::new(),
            desktops: 0,
            window_ws: HashMap::new(),
            shown: HashMap::new(),
            opacity: HashMap::new(),
            supported: Capability::ALL.into_iter().collect(),
            failures: Vec::new(),
            batches: 0,
        }
    }
}

fn contains(monitor: &MonitorInfo, x: i32, y: i32) -> bool {
    x >= monitor.size.left
        && x < monitor.size.left + monitor.size.right
        && y >= monitor.size.top
        && y < monitor.size.top + monitor.size.bottom
}

impl<X> State<X> {
    /// Maximizes a window to the work area of its monitor, or gives it back the frame it had
    fn set_maximized(&mut self, id: WindowId, maximize: bool) {
        let Some(window) = self.windows.get(&id) else {
            return;
        };
        let current = window.rect;

        if maximize {
            // the work area of the monitor the window is on (the first one if none is)
            let center = (
                current.left + current.right / 2,
                current.top + current.bottom / 2,
            );
            let area = self
                .monitors
                .iter()
                .find(|m| contains(m, center.0, center.1))
                .or_else(|| self.monitors.first())
                .map(|m| m.work_area);

            if let Some(area) = area
                && !self.maximized.contains_key(&id)
            {
                self.maximized.insert(id, current);
                if let Some(window) = self.windows.get_mut(&id) {
                    window.rect = area;
                }
            }
        } else if let Some(before) = self.maximized.remove(&id)
            && let Some(window) = self.windows.get_mut(&id)
        {
            window.rect = before;
        }
    }

    /// The monitor a window is on: the one that holds the middle of it, else the first
    fn monitor_of(&self, id: WindowId) -> Option<MonitorId> {
        let w = self.windows.get(&id)?;
        let (x, y) = (
            w.rect.left + w.rect.right / 2,
            w.rect.top + w.rect.bottom / 2,
        );
        self.monitors
            .iter()
            .find(|m| contains(m, x, y))
            .or_else(|| self.monitors.first())
            .map(|m| m.id)
    }

    /// What a window opened now would be on: the workspace its monitor is showing
    fn desktop_for_new_window(&self) -> usize {
        self.monitors
            .first()
            .and_then(|m| self.shown.get(&m.id))
            .copied()
            .unwrap_or(0)
    }

    /// Takes the oldest failure that is for this operation (and window, if it names one)
    fn take_failure(&mut self, op: Op, id: Option<WindowId>) -> Option<Error> {
        let at = self
            .failures
            .iter()
            .position(|(o, only, _)| *o == op && only.is_none_or(|only| Some(only) == id))?;
        Some(self.failures.remove(at).2)
    }

    fn require(&self, capability: Capability) -> Result<()> {
        if self.supported.contains(&capability) {
            Ok(())
        } else {
            Err(Error::Unsupported(capability))
        }
    }
}

/// An in-memory [`Backend`].
///
/// It has no windows and no monitors until a test adds them. `X` is the type of the commands a
/// platform's own extension of the fake logs; leave it out for the shared fake.
pub struct FakeBackend<X = NoExt> {
    state: Mutex<State<X>>,
}

impl<X> Default for FakeBackend<X> {
    fn default() -> Self {
        Self {
            state: Mutex::new(State::default()),
        }
    }
}

impl FakeBackend<NoExt> {
    /// Returns a fake with no monitors and no windows.
    pub fn new() -> Self {
        Self::empty()
    }

    /// Returns a fake that never refuses: a command about a window nobody opened creates it.
    ///
    /// It is for a test of pure window manager logic, which moves and focuses windows it never
    /// opened and needs no set-up.
    pub fn permissive() -> Self {
        Self::permissive_ext()
    }

    /// Returns a fake with one 1920x1080 monitor (id 1, named `Virtual-0`) whose work area leaves
    /// 30 pixels at the bottom for a panel.
    pub fn with_one_monitor() -> Self {
        Self::one_monitor()
    }
}

fn permissive_window(id: WindowId) -> WindowInfo {
    WindowInfo {
        id,
        pid: i32::try_from(id.0).unwrap_or_default() + 1000,
        rect: Rect::default(),
        minimized: false,
        alive: true,
        title: Some(format!("window {id}")),
        class: Some(format!("class.{id}")),
        exe: Some(format!("exe{id}")),
        path: Some(format!("/usr/bin/exe{id}")),
        fullscreen: Some(false),
        maximized: Some(false),
        normal: Some(true),
    }
}

impl<X: Clone + Send + 'static> FakeBackend<X> {
    fn state(&self) -> MutexGuard<'_, State<X>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns a fake with no monitors and no windows. (`FakeBackend::new` is the same for the
    /// shared fake; a platform extension with its own command type `X` has to name `X`, which
    /// the compiler cannot infer, so it calls this one.)
    pub fn empty() -> Self {
        Self::default()
    }

    /// [`FakeBackend::permissive`] for a fake with its own command type.
    pub fn permissive_ext() -> Self {
        let fake = Self::one_monitor();
        fake.state().permissive = true;
        fake
    }

    /// [`FakeBackend::with_one_monitor`] for a fake with its own command type.
    pub fn one_monitor() -> Self {
        let fake = Self::empty();
        fake.add_monitor(
            MonitorId(1),
            "Virtual-0",
            Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1050,
            },
        );
        fake
    }

    /// Unplugs a monitor. The platform takes its windows to the others by itself: a test moves
    /// them ([`FakeBackend::move_window_by_user`]) as it sees fit. Nothing is done when there is
    /// no monitor with this id.
    pub fn remove_monitor(&self, id: MonitorId) {
        self.state().monitors.retain(|m| m.id != id);
    }

    /// Gives a monitor another size and work area, as a change of resolution or scale, or a panel
    /// that moves, does. Nothing is done when there is no monitor with this id.
    pub fn resize_monitor(&self, id: MonitorId, size: Rect, work_area: Rect) {
        if let Some(monitor) = self.state().monitors.iter_mut().find(|m| m.id == id) {
            monitor.size = size;
            monitor.work_area = work_area;
        }
    }

    /// Adds a monitor. Its serial number identity is its name.
    pub fn add_monitor(&self, id: MonitorId, name: &str, size: Rect, work_area: Rect) {
        self.state().monitors.push(MonitorInfo {
            id,
            size,
            work_area,
            name: Some(name.to_string()),
            serial_number_id: Some(name.to_string()),
        });
    }

    /// Opens a window with made-up titles: `exe<id>`, class `class.<id>`, title `window <id>`.
    /// See [`FakeBackend::open_window_as`].
    pub fn open_window(&self, id: WindowId) {
        self.open_window_as(
            id,
            &format!("exe{id}"),
            &format!("class.{id}"),
            &format!("window {id}"),
        );
    }

    /// Opens a window of an application. It starts at 800x600 at (100, 100), a normal window with
    /// no focus, on the workspace the first monitor shows.
    pub fn open_window_as(&self, id: WindowId, exe: &str, class: &str, title: &str) {
        let mut state = self.state();
        let desktop = state.desktop_for_new_window();
        state.window_ws.insert(id, desktop);
        state.windows.insert(
            id,
            WindowInfo {
                id,
                pid: i32::try_from(id.0).unwrap_or_default() + 1000,
                rect: Rect {
                    left: 100,
                    top: 100,
                    right: 800,
                    bottom: 600,
                },
                minimized: false,
                alive: true,
                title: Some(title.to_string()),
                class: Some(class.to_string()),
                exe: Some(exe.to_string()),
                path: Some(format!("/usr/bin/{exe}")),
                fullscreen: Some(false),
                maximized: Some(false),
                normal: Some(true),
            },
        );
    }

    /// Destroys a window, as the user closing it does: the window manager is not asked. If it had
    /// the focus, nothing has it now.
    pub fn close_window_now(&self, id: WindowId) {
        let mut state = self.state();
        if let Some(window) = state.windows.get_mut(&id) {
            window.alive = false;
        }
        if state.foreground == Some(id) {
            state.foreground = None;
        }
    }

    /// Marks a window as a dialog, a splash screen, a utility window or the like, or as a normal
    /// one again.
    pub fn set_normal(&self, id: WindowId, normal: bool) {
        if let Some(w) = self.state().windows.get_mut(&id) {
            w.normal = Some(normal);
        }
    }

    /// Puts a window somewhere without the window manager having asked, as a user dragging it
    /// does. The frame is exactly this one: no minimum size and no frame offset apply.
    pub fn move_window_by_user(&self, id: WindowId, rect: Rect) {
        if let Some(w) = self.state().windows.get_mut(&id) {
            w.rect = rect;
        }
    }

    /// Gives a window a minimum size. A [`Backend::set_rect`] for something smaller leaves it at
    /// the minimum, which is how a GTK4 client answers.
    pub fn set_min_size(&self, id: WindowId, width: i32, height: i32) {
        self.state().min_sizes.insert(id, (width, height));
    }

    /// Gives every window a [`FrameOffset`]: what it ends up as, compared with what it was asked
    /// to be. A window with an offset of its own ([`FakeBackend::set_window_frame_offset`]) keeps
    /// that one. Windows already placed stay where they are.
    pub fn set_frame_offset(&self, offset: FrameOffset) {
        self.state().frame_offset = offset;
    }

    /// Gives one window a [`FrameOffset`] of its own.
    pub fn set_window_frame_offset(&self, id: WindowId, offset: FrameOffset) {
        self.state().window_frame_offsets.insert(id, offset);
    }

    /// Changes a window's title.
    pub fn set_title(&self, id: WindowId, title: &str) {
        if let Some(w) = self.state().windows.get_mut(&id) {
            w.title = Some(title.to_string());
        }
    }

    /// Changes which window has the focus, as the user does, without the window manager asking.
    pub fn set_foreground(&self, id: Option<WindowId>) {
        self.state().foreground = id;
    }

    /// Puts the pointer somewhere.
    pub fn set_cursor(&self, x: i32, y: i32) {
        self.state().cursor = Some((x, y));
    }

    /// Maximizes or restores a window as the user does with its title bar: the platform does it
    /// and nobody asks the window manager, so no command is recorded. A test sends the event that
    /// says so.
    pub fn user_maximize(&self, id: WindowId, maximize: bool) {
        self.state().set_maximized(id, maximize);
    }

    /// Puts a window in or out of fullscreen as a platform does, without the window manager
    /// being asked, so no command is recorded: the window covers its monitor, and when it leaves
    /// fullscreen it gets the frame it had before, which may not be where the window manager
    /// would put it now. A test sends the event that says so.
    pub fn set_fullscreen(&self, id: WindowId, fullscreen: bool) {
        let mut state = self.state();
        let covers = state
            .monitor_of(id)
            .and_then(|m| state.monitors.iter().find(|monitor| monitor.id == m))
            .map(|monitor| monitor.size);

        if fullscreen {
            let Some(window) = state.windows.get_mut(&id) else {
                return;
            };
            let before = window.rect;
            window.fullscreen = Some(true);
            if let Some(covers) = covers {
                window.rect = covers;
            }
            state.fullscreen.insert(id, before);
        } else if let Some(before) = state.fullscreen.remove(&id)
            && let Some(window) = state.windows.get_mut(&id)
        {
            window.fullscreen = Some(false);
            window.rect = before;
        }
    }

    /// Returns true if and only if the window is maximized, by a command or by
    /// [`FakeBackend::user_maximize`], and has not been restored since.
    pub fn is_maximized(&self, id: WindowId) -> bool {
        self.state().maximized.contains_key(&id)
    }

    /// Returns where a window is, if it was ever opened.
    pub fn rect_of(&self, id: WindowId) -> Option<Rect> {
        self.state().windows.get(&id).map(|w| w.rect)
    }

    /// Returns true if and only if the window is hidden ([`Backend::hide`]), as opposed to being
    /// on a workspace that is not showing.
    pub fn is_hidden(&self, id: WindowId) -> bool {
        self.state().hidden.contains(&id)
    }

    /// Returns true if and only if the window can be seen: it is alive, not minimized, not hidden
    /// and on the workspace its monitor shows.
    pub fn on_screen(&self, id: WindowId) -> bool {
        let state = self.state();
        let Some(window) = state.windows.get(&id) else {
            return false;
        };
        let shown = state
            .monitor_of(id)
            .and_then(|m| state.shown.get(&m))
            .copied()
            .unwrap_or(0);
        window.alive
            && !window.minimized
            && !state.hidden.contains(&id)
            && state.window_ws.get(&id).copied().unwrap_or(0) == shown
    }

    /// Returns the workspace a window is on, if it was ever opened.
    pub fn workspace_of(&self, id: WindowId) -> Option<usize> {
        self.state().window_ws.get(&id).copied()
    }

    /// Returns the workspace a monitor shows.
    pub fn shown_workspace(&self, monitor: MonitorId) -> usize {
        self.state().shown.get(&monitor).copied().unwrap_or(0)
    }

    /// Shows workspace `idx` on a monitor the way something other than komorebi does (a pager, a
    /// shortcut, a click on a window of another desktop): no command is recorded, and the
    /// platform's event about it is the test's to send.
    pub fn show_workspace_by_user(&self, monitor: MonitorId, idx: usize) {
        let mut state = self.state();
        state.desktops = state.desktops.max(idx + 1);
        state.shown.insert(monitor, idx);
    }

    /// Returns how opaque the window manager last asked this window to be, 255 being not
    /// see-through at all, which is what a window is until it is asked otherwise.
    pub fn opacity_of(&self, id: WindowId) -> u8 {
        self.state().opacity.get(&id).copied().unwrap_or(255)
    }

    /// Returns every command received so far, oldest first.
    pub fn commands(&self) -> Vec<Command<X>> {
        self.state().commands.clone()
    }

    /// Forgets the commands received so far.
    pub fn clear_commands(&self) {
        self.state().commands.clear();
    }

    /// Logs a command of a platform's own extension of the fake, in its place among the others.
    pub fn record_ext(&self, command: X) {
        self.state().commands.push(Command::Ext(command));
    }

    /// Says whether the fake has this [`Capability`]. A fake starts with all of them. Without
    /// one, its operations return [`Error::Unsupported`], are not recorded as commands and
    /// change nothing.
    pub fn set_supported(&self, capability: Capability, supported: bool) {
        let mut state = self.state();
        if supported {
            state.supported.insert(capability);
        } else {
            state.supported.remove(&capability);
        }
    }

    /// Makes the next call of `op` fail with `error`, whichever window it is about. Failures are
    /// handed out oldest first, one per call. The call is recorded as a command and changes
    /// nothing else.
    pub fn fail_next(&self, op: Op, error: Error) {
        self.state().failures.push((op, None, error));
    }

    /// Makes the next call of `op` about this window fail with `error`; calls about other
    /// windows are not affected, and do not use it up.
    pub fn fail_next_for(&self, op: Op, id: WindowId, error: Error) {
        self.state().failures.push((op, Some(id), error));
    }

    /// Returns how many injected failures have not been handed out yet.
    pub fn pending_failures(&self) -> usize {
        self.state().failures.len()
    }

    /// Forgets the failures that have not been handed out.
    pub fn clear_failures(&self) {
        self.state().failures.clear();
    }

    /// Returns how many batches are open: [`Backend::begin_batch`] without its
    /// [`Backend::end_batch`] yet.
    pub fn open_batches(&self) -> usize {
        self.state().batches
    }

    /// Records a command about a window, hands out an injected failure if one is due, and
    /// otherwise runs `f` on a live window.
    fn record<T>(
        &self,
        id: WindowId,
        op: Op,
        command: Command<X>,
        f: impl FnOnce(&mut State<X>) -> T,
    ) -> Result<T> {
        let mut state = self.state();
        state.commands.push(command);
        if let Some(error) = state.take_failure(op, Some(id)) {
            return Err(error);
        }
        if state.permissive && !state.windows.contains_key(&id) {
            state.windows.insert(id, permissive_window(id));
        }
        match state.windows.get(&id) {
            Some(w) if w.alive => Ok(f(&mut state)),
            _ => Err(Error::WindowGone(id)),
        }
    }

    /// Like [`FakeBackend::record`], for a command that names no window, and for which `f`
    /// cannot fail.
    fn record_global<T>(
        &self,
        op: Op,
        command: Command<X>,
        f: impl FnOnce(&mut State<X>) -> T,
    ) -> Result<T> {
        let mut state = self.state();
        state.commands.push(command);
        if let Some(error) = state.take_failure(op, None) {
            return Err(error);
        }
        Ok(f(&mut state))
    }

    /// Like [`FakeBackend::record`], for an operation of a [`Capability`].
    fn record_capable<T>(
        &self,
        capability: Capability,
        id: WindowId,
        op: Op,
        command: Command<X>,
        f: impl FnOnce(&mut State<X>) -> T,
    ) -> Result<T> {
        self.state().require(capability)?;
        self.record(id, op, command, f)
    }

    fn record_capable_global<T>(
        &self,
        capability: Capability,
        op: Op,
        command: Command<X>,
        f: impl FnOnce(&mut State<X>) -> T,
    ) -> Result<T> {
        self.state().require(capability)?;
        self.record_global(op, command, f)
    }
}

impl<X: Clone + Send + 'static> Backend for FakeBackend<X> {
    fn windows(&self) -> Vec<WindowInfo> {
        let state = self.state();
        let mut windows: Vec<_> = state
            .windows
            .values()
            .filter(|w| w.alive)
            .cloned()
            .map(|mut w| {
                w.maximized = Some(state.maximized.contains_key(&w.id));
                w
            })
            .collect();
        windows.sort_by_key(|w| w.id);
        windows
    }

    fn window(&self, id: WindowId) -> Option<WindowInfo> {
        let mut state = self.state();
        if state.permissive && !state.windows.contains_key(&id) {
            state.windows.insert(id, permissive_window(id));
        }
        let maximized = state.maximized.contains_key(&id);
        state.windows.get(&id).cloned().map(|mut w| {
            w.maximized = Some(maximized);
            w
        })
    }

    fn foreground_window(&self) -> Option<WindowId> {
        self.state().foreground
    }

    fn monitors(&self) -> Result<Vec<MonitorInfo>> {
        let mut state = self.state();
        if let Some(error) = state.take_failure(Op::Monitors, None) {
            return Err(error);
        }
        Ok(state.monitors.clone())
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        self.state().cursor
    }

    fn set_rect(&self, id: WindowId, rect: &Rect) -> Result<()> {
        let rect = *rect;
        self.record(id, Op::SetRect, Command::SetRect(id, rect), |state| {
            let (min_w, min_h) = state.min_sizes.get(&id).copied().unwrap_or((0, 0));
            let offset = state
                .window_frame_offsets
                .get(&id)
                .copied()
                .unwrap_or(state.frame_offset);
            if let Some(w) = state.windows.get_mut(&id) {
                w.rect = offset.apply(Rect {
                    left: rect.left,
                    top: rect.top,
                    right: rect.right.max(min_w),
                    bottom: rect.bottom.max(min_h),
                });
            }
        })
    }

    fn focus(&self, id: WindowId, mouse_follows_focus: bool) -> Result<()> {
        self.record(
            id,
            Op::Focus,
            Command::Focus(id, mouse_follows_focus),
            |state| {
                state.foreground = Some(id);
                if mouse_follows_focus && let Some(w) = state.windows.get(&id) {
                    state.cursor = Some((
                        w.rect.left + w.rect.right / 2,
                        w.rect.top + w.rect.bottom / 2,
                    ));
                }
            },
        )
    }

    fn raise(&self, id: WindowId) -> Result<()> {
        self.record(id, Op::Raise, Command::Raise(id), |_| ())
    }

    fn hide(&self, id: WindowId) -> Result<()> {
        self.record(id, Op::Hide, Command::Hide(id), |state| {
            if !state.hidden.contains(&id) {
                state.hidden.push(id);
            }
        })
    }

    fn restore(&self, id: WindowId) -> Result<()> {
        self.record(id, Op::Restore, Command::Restore(id), |state| {
            state.hidden.retain(|h| *h != id);
        })
    }

    fn minimize(&self, id: WindowId) -> Result<()> {
        self.record(id, Op::Minimize, Command::Minimize(id), |state| {
            if let Some(w) = state.windows.get_mut(&id) {
                w.minimized = true;
            }
        })
    }

    fn unminimize(&self, id: WindowId) -> Result<()> {
        self.record(id, Op::Unminimize, Command::Unminimize(id), |state| {
            if let Some(w) = state.windows.get_mut(&id) {
                w.minimized = false;
            }
        })
    }

    fn close(&self, id: WindowId) -> Result<()> {
        self.record(id, Op::Close, Command::Close(id), |state| {
            if let Some(w) = state.windows.get_mut(&id) {
                w.alive = false;
            }
        })
    }

    fn center_cursor_in(&self, rect: &Rect) -> Result<()> {
        let rect = *rect;
        self.record_global(Op::CenterCursor, Command::CenterCursor(rect), |state| {
            state.cursor = Some((rect.left + rect.right / 2, rect.top + rect.bottom / 2));
        })
    }

    fn supports(&self, capability: Capability) -> bool {
        self.state().supported.contains(&capability)
    }

    fn maximize(&self, id: WindowId, maximize: bool) -> Result<()> {
        self.record_capable(
            Capability::Maximize,
            id,
            Op::Maximize,
            Command::Maximize(id, maximize),
            |state| state.set_maximized(id, maximize),
        )
    }

    fn set_opacity(&self, id: WindowId, alpha: u8) -> Result<()> {
        self.record_capable(
            Capability::Opacity,
            id,
            Op::SetOpacity,
            Command::SetOpacity(id, alpha),
            |state| {
                state.opacity.insert(id, alpha);
            },
        )
    }

    fn set_focus_follows_mouse(&self, enable: bool) -> Result<()> {
        self.record_capable_global(
            Capability::FocusFollowsMouse,
            Op::SetFocusFollowsMouse,
            Command::FocusFollowsMouse(enable),
            |_| (),
        )
    }

    fn ensure_workspaces(&self, names: &[String]) -> Result<()> {
        let count = names.len();
        self.record_capable_global(
            Capability::Workspaces,
            Op::EnsureWorkspaces,
            Command::EnsureWorkspaces(count),
            |state| state.desktops = state.desktops.max(count),
        )
    }

    fn switch_workspace(&self, monitor: MonitorId, idx: usize) -> Result<()> {
        self.record_capable_global(
            Capability::Workspaces,
            Op::SwitchWorkspace,
            Command::SwitchWorkspace(monitor, idx),
            |state| {
                state.desktops = state.desktops.max(idx + 1);
                state.shown.insert(monitor, idx);
            },
        )
    }

    fn set_window_workspace(&self, id: WindowId, idx: usize) -> Result<()> {
        self.record_capable(
            Capability::Workspaces,
            id,
            Op::SetWindowWorkspace,
            Command::SetWindowWorkspace(id, idx),
            |state| {
                state.desktops = state.desktops.max(idx + 1);
                state.window_ws.insert(id, idx);
            },
        )
    }

    fn window_workspace(&self, id: WindowId) -> Result<Option<usize>> {
        let mut state = self.state();
        state.require(Capability::Workspaces)?;
        if let Some(error) = state.take_failure(Op::WindowWorkspace, Some(id)) {
            return Err(error);
        }
        if state.permissive && !state.windows.contains_key(&id) {
            state.windows.insert(id, permissive_window(id));
        }
        Ok(state.window_ws.get(&id).copied())
    }

    fn current_workspaces(&self) -> Result<Vec<(MonitorId, usize)>> {
        let mut state = self.state();
        state.require(Capability::Workspaces)?;
        if let Some(error) = state.take_failure(Op::CurrentWorkspaces, None) {
            return Err(error);
        }
        Ok(state
            .monitors
            .iter()
            .map(|m| (m.id, state.shown.get(&m.id).copied().unwrap_or(0)))
            .collect())
    }

    fn begin_batch(&self) {
        let mut state = self.state();
        state.batches += 1;
        state.commands.push(Command::BatchBegin);
    }

    fn end_batch(&self) {
        let mut state = self.state();
        state.batches = state.batches.saturating_sub(1);
        state.commands.push(Command::BatchEnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: WindowId = WindowId(1);
    const TWO: WindowId = WindowId(2);
    const MONITOR: MonitorId = MonitorId(1);

    fn tile(left: i32, width: i32) -> Rect {
        Rect {
            left,
            top: 0,
            right: width,
            bottom: 1050,
        }
    }

    fn fake_with(ids: &[u64]) -> FakeBackend {
        let fake = FakeBackend::with_one_monitor();
        for id in ids {
            fake.open_window(WindowId(*id));
        }
        fake
    }

    #[test]
    fn a_window_asked_to_move_is_where_it_was_asked() -> Result<()> {
        let fake = fake_with(&[1]);

        fake.set_rect(ONE, &tile(10, 500))?;

        assert_eq!(fake.rect_of(ONE), Some(tile(10, 500)));
        assert_eq!(fake.window_rect(ONE), Some(tile(10, 500)));
        Ok(())
    }

    #[test]
    fn frame_is_the_requested_rect_and_an_unknown_window_is_gone() -> Result<()> {
        let fake = fake_with(&[1]);

        fake.set_rect(ONE, &tile(10, 500))?;

        assert_eq!(fake.frame(ONE)?, tile(10, 500));
        assert!(fake.frame(WindowId(99)).is_err_and(|e| e.is_window_gone()));
        Ok(())
    }

    #[test]
    fn a_window_keeps_to_its_minimum_size() -> Result<()> {
        let fake = fake_with(&[1]);
        fake.set_min_size(ONE, 600, 400);

        fake.set_rect(ONE, &tile(0, 300))?;

        let rect = fake.rect_of(ONE).expect("window 1 was opened");
        assert_eq!((rect.right, rect.bottom), (600, 1050));
        Ok(())
    }

    #[test]
    fn every_command_is_recorded_in_order() -> Result<()> {
        let fake = fake_with(&[1, 2]);

        fake.focus(ONE, false)?;
        fake.raise(TWO)?;
        fake.minimize(ONE)?;
        fake.unminimize(ONE)?;
        fake.close(TWO)?;

        assert_eq!(
            fake.commands(),
            [
                Command::Focus(ONE, false),
                Command::Raise(TWO),
                Command::Minimize(ONE),
                Command::Unminimize(ONE),
                Command::Close(TWO),
            ]
        );
        fake.clear_commands();
        assert!(fake.commands().is_empty());
        Ok(())
    }

    #[test]
    fn a_maximized_window_has_the_work_area_and_gets_its_frame_back() -> Result<()> {
        let fake = fake_with(&[1]);
        fake.set_rect(ONE, &tile(100, 200))?;

        fake.maximize(ONE, true)?;
        // a second maximize does not make the maximized frame the one to give back
        fake.maximize(ONE, true)?;

        assert!(fake.is_maximized(ONE));
        assert_eq!(fake.rect_of(ONE), Some(tile(0, 1920)));
        assert_eq!(fake.window(ONE).and_then(|w| w.maximized), Some(true));

        fake.maximize(ONE, false)?;

        assert!(!fake.is_maximized(ONE));
        assert_eq!(fake.rect_of(ONE), Some(tile(100, 200)));
        Ok(())
    }

    #[test]
    fn a_window_the_user_maximizes_is_maximized_and_no_command_is_recorded() {
        let fake = fake_with(&[1]);
        fake.set_rect(ONE, &tile(100, 200)).unwrap();
        fake.clear_commands();

        fake.user_maximize(ONE, true);

        assert!(fake.is_maximized(ONE));
        assert_eq!(fake.windows()[0].maximized, Some(true));
        assert_eq!(fake.rect_of(ONE), Some(tile(0, 1920)));
        assert!(fake.commands().is_empty());

        fake.user_maximize(ONE, false);

        assert_eq!(fake.window(ONE).and_then(|w| w.maximized), Some(false));
        assert_eq!(fake.rect_of(ONE), Some(tile(100, 200)));
    }

    #[test]
    fn a_fullscreen_window_covers_its_monitor_and_gets_its_frame_back() {
        let fake = fake_with(&[1]);
        fake.set_rect(ONE, &tile(100, 200)).unwrap();

        fake.set_fullscreen(ONE, true);

        assert_eq!(fake.windows()[0].fullscreen, Some(true));
        assert_eq!(
            fake.rect_of(ONE),
            Some(Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }),
            "the whole monitor, panel included"
        );

        fake.set_fullscreen(ONE, false);

        assert_eq!(fake.window(ONE).and_then(|w| w.fullscreen), Some(false));
        assert_eq!(fake.rect_of(ONE), Some(tile(100, 200)));
    }

    #[test]
    fn a_command_about_a_window_nobody_opened_or_that_closed_is_window_gone() {
        let fake = fake_with(&[1]);
        fake.close_window_now(ONE);

        let unknown = fake.focus(WindowId(9), false).expect_err("no window 9");
        let closed = fake.raise(ONE).expect_err("window 1 closed");

        assert!(unknown.is_window_gone() && closed.is_window_gone());
        assert_eq!(unknown.to_string(), "no such window: 9");
    }

    #[test]
    fn a_permissive_fake_creates_the_windows_it_is_asked_about() -> Result<()> {
        let fake = FakeBackend::permissive();

        fake.focus(WindowId(77), false)?;

        assert!(fake.is_window(WindowId(77)));
        assert_eq!(fake.foreground_window(), Some(WindowId(77)));
        Ok(())
    }

    #[test]
    fn focusing_with_the_mouse_following_puts_the_pointer_in_the_middle_of_the_window() -> Result<()>
    {
        let fake = fake_with(&[1]);
        fake.set_rect(ONE, &tile(100, 200))?;

        fake.focus(ONE, true)?;
        assert_eq!(fake.cursor_pos(), Some((200, 525)));

        fake.set_cursor(5, 5);
        fake.focus(ONE, false)?;
        assert_eq!(fake.cursor_pos(), Some((5, 5)));
        Ok(())
    }

    #[test]
    fn what_the_user_does_is_not_a_command() {
        let fake = fake_with(&[1]);

        fake.move_window_by_user(ONE, tile(300, 300));
        fake.set_title(ONE, "renamed");
        fake.set_normal(ONE, false);

        assert!(fake.commands().is_empty());
        assert_eq!(fake.rect_of(ONE), Some(tile(300, 300)));
        let window = fake.window(ONE).expect("window 1 was opened");
        assert_eq!(window.title.as_deref(), Some("renamed"));
        assert_eq!(window.normal, Some(false));
    }

    #[test]
    fn windows_are_listed_by_id_and_a_destroyed_one_is_left_out() {
        let fake = fake_with(&[3, 1, 2]);
        fake.close_window_now(TWO);

        let ids: Vec<_> = fake.windows().iter().map(|w| w.id).collect();

        assert_eq!(ids, [ONE, WindowId(3)]);
    }

    #[test]
    fn a_monitor_shows_the_windows_of_its_workspace_and_no_others() -> Result<()> {
        let fake = fake_with(&[1, 2]);
        fake.set_window_workspace(TWO, 1)?;

        assert!(fake.on_screen(ONE) && !fake.on_screen(TWO));

        fake.switch_workspace(MONITOR, 1)?;

        assert!(!fake.on_screen(ONE) && fake.on_screen(TWO));
        assert_eq!(fake.shown_workspace(MONITOR), 1);
        assert_eq!(fake.current_workspaces()?, [(MONITOR, 1)]);
        assert_eq!(fake.window_workspace(TWO)?, Some(1));
        assert_eq!(
            fake.commands(),
            [
                Command::SetWindowWorkspace(TWO, 1),
                Command::SwitchWorkspace(MONITOR, 1)
            ]
        );
        Ok(())
    }

    #[test]
    fn a_hidden_window_is_out_of_sight_on_its_workspace_until_it_is_restored() -> Result<()> {
        let fake = fake_with(&[1]);

        fake.hide(ONE)?;
        fake.hide(ONE)?;

        assert!(fake.is_hidden(ONE) && !fake.on_screen(ONE));
        assert_eq!(fake.window_workspace(ONE)?, Some(0));
        assert!(fake.window(ONE).is_some_and(|w| w.alive && !w.minimized));

        fake.restore(ONE)?;

        assert!(!fake.is_hidden(ONE) && fake.on_screen(ONE));
        Ok(())
    }

    #[test]
    fn the_cursor_can_be_centered_in_a_rectangle() -> Result<()> {
        let fake = FakeBackend::new();

        fake.center_cursor_in(&tile(0, 400))?;

        assert_eq!(fake.cursor_pos(), Some((200, 525)));
        assert_eq!(fake.commands(), [Command::CenterCursor(tile(0, 400))]);
        Ok(())
    }

    #[test]
    fn an_injected_failure_is_handed_out_once_and_the_command_is_still_recorded() {
        let fake = fake_with(&[1]);
        fake.fail_next(Op::SetRect, Error::platform("refused"));

        let error = fake.set_rect(ONE, &tile(0, 100)).expect_err("injected");

        assert_eq!(error.to_string(), "refused");
        assert_eq!(
            fake.rect_of(ONE).map(|r| r.left),
            Some(100),
            "nothing moved"
        );
        assert_eq!(fake.pending_failures(), 0);
        fake.set_rect(ONE, &tile(0, 100))
            .expect("the failure was used up");
        assert_eq!(
            fake.commands(),
            [
                Command::SetRect(ONE, tile(0, 100)),
                Command::SetRect(ONE, tile(0, 100))
            ]
        );
    }

    #[test]
    fn failures_are_handed_out_oldest_first_and_per_operation() -> Result<()> {
        let fake = fake_with(&[1]);
        fake.fail_next(Op::Hide, Error::platform("first"));
        fake.fail_next(Op::Hide, Error::platform("second"));
        fake.fail_next(Op::Raise, Error::platform("raise"));

        assert_eq!(fake.hide(ONE).unwrap_err().to_string(), "first");
        assert_eq!(fake.raise(ONE).unwrap_err().to_string(), "raise");
        assert_eq!(fake.hide(ONE).unwrap_err().to_string(), "second");
        fake.hide(ONE)?;
        Ok(())
    }

    #[test]
    fn a_failure_for_one_window_leaves_the_others_and_is_not_used_up_by_them() -> Result<()> {
        let fake = fake_with(&[1, 2]);
        fake.fail_next_for(Op::Close, TWO, Error::WindowGone(TWO));

        fake.close(ONE)?;
        assert_eq!(fake.pending_failures(), 1);
        assert!(fake.close(TWO).unwrap_err().is_window_gone());
        assert_eq!(fake.pending_failures(), 0);
        Ok(())
    }

    #[test]
    fn a_failure_can_be_injected_into_the_monitor_query_and_cleared() {
        let fake = fake_with(&[]);
        fake.fail_next(Op::Monitors, Error::platform("no outputs"));
        fake.fail_next(Op::Focus, Error::platform("never"));

        assert!(fake.monitors().is_err());
        assert!(fake.monitors().is_ok());
        fake.clear_failures();
        assert_eq!(fake.pending_failures(), 0);
    }

    #[test]
    fn a_frame_offset_moves_the_window_from_what_it_was_asked() -> Result<()> {
        let fake = fake_with(&[1, 2]);
        fake.set_frame_offset(FrameOffset {
            left: -7,
            top: 0,
            right: 14,
            bottom: 7,
        });
        fake.set_window_frame_offset(TWO, FrameOffset::default());

        fake.set_rect(ONE, &tile(100, 400))?;
        fake.set_rect(TWO, &tile(100, 400))?;

        assert_eq!(
            fake.window_rect(ONE),
            Some(Rect {
                left: 93,
                top: 0,
                right: 414,
                bottom: 1057
            })
        );
        assert_eq!(
            fake.window_rect(TWO),
            Some(tile(100, 400)),
            "its own offset"
        );
        Ok(())
    }

    #[test]
    fn a_capability_that_is_switched_off_is_unsupported_and_changes_nothing() {
        let fake = fake_with(&[1]);
        for capability in Capability::ALL {
            assert!(fake.supports(capability));
        }
        fake.set_supported(Capability::Maximize, false);
        fake.set_supported(Capability::Workspaces, false);

        assert!(!fake.supports(Capability::Maximize));
        assert!(fake.maximize(ONE, true).unwrap_err().is_unsupported());
        assert!(
            fake.switch_workspace(MONITOR, 1)
                .unwrap_err()
                .is_unsupported()
        );
        assert!(fake.window_workspace(ONE).unwrap_err().is_unsupported());
        assert!(fake.commands().is_empty());
        assert!(fake.set_opacity(ONE, 10).is_ok());
    }

    #[test]
    fn a_batch_is_counted_and_logged() {
        let fake = fake_with(&[1]);
        {
            let _outer = crate::Batch::begin(&fake);
            let _inner = crate::Batch::begin(&fake);
            assert_eq!(fake.open_batches(), 2);
        }
        assert_eq!(fake.open_batches(), 0);
        assert_eq!(
            fake.commands(),
            [
                Command::BatchBegin,
                Command::BatchBegin,
                Command::BatchEnd,
                Command::BatchEnd
            ]
        );
    }

    #[test]
    fn an_extension_command_keeps_its_place_in_the_log() -> Result<()> {
        let fake: FakeBackend<&'static str> = FakeBackend::empty();
        fake.add_monitor(MONITOR, "m", tile(0, 100), tile(0, 100));
        fake.open_window(ONE);

        fake.raise(ONE)?;
        fake.record_ext("look");
        fake.hide(ONE)?;

        assert_eq!(
            fake.commands(),
            [
                Command::Raise(ONE),
                Command::Ext("look"),
                Command::Hide(ONE)
            ]
        );
        Ok(())
    }
}
