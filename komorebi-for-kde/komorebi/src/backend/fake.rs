//! An in-memory desktop, so that window manager logic can be tested without a compositor.
//!
//! A [`FakeBackend`] does what it is told, at once and exactly: a window asked to move to a
//! rectangle is at that rectangle, unless it was given a minimum size, which it then keeps to the
//! way a GTK4 client does. Every command is recorded, so that a test can assert on what the window
//! manager asked for, and a test can do what a user does (move a window, close it, change which
//! one has the focus) without the window manager having asked.
//!
//! # Example
//!
//! ```rust
//! use komorebi::backend::Backend;
//! use komorebi::backend::fake::Command;
//! use komorebi::backend::fake::FakeBackend;
//! use komorebi::core::Rect;
//!
//! let fake = FakeBackend::with_one_monitor();
//! fake.open_window(1, "foot", "foot", "a terminal");
//!
//! let tile = Rect { left: 0, top: 0, right: 960, bottom: 1050 };
//! fake.set_rect(1.into(), &tile)?;
//!
//! assert_eq!(fake.rect_of(1), Some(tile));
//! assert_eq!(fake.commands(), [Command::SetRect(1, tile)]);
//! # Ok::<(), color_eyre::eyre::Report>(())
//! ```

//!
//! # Built on the shared fake
//!
//! The desktop itself is `komorebi_backend::fake::FakeBackend`, which every komorebi shares, and
//! this type puts komorebi for KDE's names on it: ids are `u32`, errors are `eyre` reports (the
//! text of a command about a window that is gone is `no such window: <id>`), and the commands
//! that only the KDE backend has (the look, the border colors, the wallpaper, the frames of a
//! drag) are kept here, in the order they were given among the shared ones.

#![deny(missing_docs)]

use super::Backend;
use super::Look;
use super::MonitorInfo;
use super::WindowInfo;
use crate::core::Rect;
use color_eyre::eyre;
use komorebi_backend as shared;
use komorebi_backend::MonitorId;
use komorebi_backend::WindowId;
use komorebi_backend::fake::Command as SharedCommand;
use parking_lot::Mutex;
use shared::Backend as SharedBackend;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// A command the window manager sent to the backend.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// [`Backend::set_rect`]: the window and the rectangle it was asked to take.
    SetRect(u32, Rect),
    /// [`Backend::focus`]: the window, and whether the pointer was to follow.
    Focus(u32, bool),
    /// [`Backend::raise`].
    Raise(u32),
    /// [`Backend::hide`].
    Hide(u32),
    /// [`Backend::restore`].
    Restore(u32),
    /// [`Backend::minimize`].
    Minimize(u32),
    /// [`Backend::unminimize`].
    Unminimize(u32),
    /// [`Backend::close`].
    Close(u32),
    /// [`Backend::center_cursor_in`].
    CenterCursor(Rect),
    /// [`Backend::ensure_workspaces`]: how many workspaces were asked for.
    EnsureWorkspaces(usize),
    /// [`Backend::maximize`]: the window, and whether it was to be maximized or restored.
    Maximize(u32, bool),
    /// [`Backend::switch_workspace`]: the monitor, and the workspace it was asked to show.
    SwitchWorkspace(u32, usize),
    /// [`Backend::set_window_workspace`]: the window, and the workspace it was put on.
    SetWindowWorkspace(u32, usize),
    /// [`Backend::apply_look`]: the look, when it is not the one applied before.
    ApplyLook(Look),
    /// [`Backend::set_border_colours`]: the colors of each window's border, when they are not the
    /// ones given before.
    BorderColours(Vec<(u32, u32)>),
    /// [`Backend::set_opacity`]: the window, and how opaque it was asked to be.
    SetOpacity(u32, u8),
    /// [`Backend::set_focus_follows_mouse`]: whether it was turned on or off.
    FocusFollowsMouse(bool),
    /// [`Backend::set_wallpaper`]: the monitor, and the picture it was asked to show.
    SetWallpaper(u32, PathBuf),
}

/// The commands only the KDE backend has: they go in the shared fake's log (so that they keep
/// their place among the shared ones) and come out as [`Command`]s.
#[derive(Clone, Debug, PartialEq)]
enum KdeCommand {
    ApplyLook(Look),
    BorderColours(Vec<(u32, u32)>),
    SetWallpaper(u32, PathBuf),
}

#[derive(Default)]
struct KdeState {
    /// the frame the user asked for when they last let go of an edge of each window
    drag_ends: HashMap<u32, Rect>,
    /// the border color each window was last given, `0xRRGGBB`
    border_colours: HashMap<u32, u32>,
    /// the wallpaper the WM last asked for on each monitor
    wallpapers: HashMap<u32, PathBuf>,
    /// the look last applied, so that asking for it again is not a command
    last_look: Option<Look>,
    /// whether the compositor has an effect that draws borders
    borders_drawn: bool,
}

/// The id of a window of this backend: always one that was made from a `u32`.
fn raw(id: WindowId) -> u32 {
    id.to_u32().unwrap_or(u32::MAX)
}

fn raw_monitor(id: MonitorId) -> u32 {
    id.to_u32().unwrap_or(u32::MAX)
}

fn wid(id: u32) -> WindowId {
    WindowId::from_u32(id)
}

fn window_info(info: shared::WindowInfo) -> WindowInfo {
    WindowInfo {
        id: raw(info.id),
        pid: info.pid,
        title: info.title.unwrap_or_default(),
        class: info.class.unwrap_or_default(),
        exe: info.exe.unwrap_or_default(),
        path: info.path.unwrap_or_default(),
        rect: info.rect,
        minimized: info.minimized,
        fullscreen: info.fullscreen.unwrap_or_default(),
        maximized: info.maximized.unwrap_or_default(),
        normal: info.normal.unwrap_or(true),
        alive: info.alive,
    }
}

fn command(command: SharedCommand<KdeCommand>) -> Option<Command> {
    Some(match command {
        SharedCommand::SetRect(id, rect) => Command::SetRect(raw(id), rect),
        SharedCommand::Focus(id, follow) => Command::Focus(raw(id), follow),
        SharedCommand::Raise(id) => Command::Raise(raw(id)),
        SharedCommand::Hide(id) => Command::Hide(raw(id)),
        SharedCommand::Restore(id) => Command::Restore(raw(id)),
        SharedCommand::Minimize(id) => Command::Minimize(raw(id)),
        SharedCommand::Unminimize(id) => Command::Unminimize(raw(id)),
        SharedCommand::Close(id) => Command::Close(raw(id)),
        SharedCommand::CenterCursor(rect) => Command::CenterCursor(rect),
        SharedCommand::EnsureWorkspaces(count) => Command::EnsureWorkspaces(count),
        SharedCommand::Maximize(id, on) => Command::Maximize(raw(id), on),
        SharedCommand::SwitchWorkspace(monitor, idx) => {
            Command::SwitchWorkspace(raw_monitor(monitor), idx)
        }
        SharedCommand::SetWindowWorkspace(id, idx) => Command::SetWindowWorkspace(raw(id), idx),
        SharedCommand::SetOpacity(id, alpha) => Command::SetOpacity(raw(id), alpha),
        SharedCommand::FocusFollowsMouse(on) => Command::FocusFollowsMouse(on),
        SharedCommand::Ext(KdeCommand::ApplyLook(look)) => Command::ApplyLook(look),
        SharedCommand::Ext(KdeCommand::BorderColours(colours)) => Command::BorderColours(colours),
        SharedCommand::Ext(KdeCommand::SetWallpaper(monitor, path)) => {
            Command::SetWallpaper(monitor, path)
        }
        // KDE's trait never opens a batch
        SharedCommand::BatchBegin | SharedCommand::BatchEnd => return None,
    })
}

/// An in-memory [`Backend`].
///
/// It has no windows and no monitors until a test adds them. The window manager reaches it through
/// [`crate::backend::backend`], installed with [`crate::backend::with_backend`].
pub struct FakeBackend {
    inner: shared::fake::FakeBackend<KdeCommand>,
    kde: Mutex<KdeState>,
}

impl Default for FakeBackend {
    fn default() -> Self {
        Self {
            inner: shared::fake::FakeBackend::empty(),
            kde: Mutex::new(KdeState::default()),
        }
    }
}

impl FakeBackend {
    /// Returns a fake with no monitors and no windows.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a fake that never refuses: a command about a window nobody opened creates it.
    ///
    /// The crate's own unit tests get one by default, so that a test of pure window manager logic,
    /// which moves and focuses windows it never opened, needs no set-up.
    pub fn permissive() -> Self {
        Self {
            inner: shared::fake::FakeBackend::permissive_ext(),
            kde: Mutex::new(KdeState::default()),
        }
    }

    /// Returns a fake with one 1920x1080 monitor, named `Virtual-0`, whose work area leaves 30
    /// pixels at the bottom for a panel.
    pub fn with_one_monitor() -> Self {
        Self {
            inner: shared::fake::FakeBackend::one_monitor(),
            kde: Mutex::new(KdeState::default()),
        }
    }

    /// Unplugs a monitor. The compositor takes its windows to the others by itself: a test moves
    /// them (`move_window_by_user`) as it sees fit. Nothing is done when there is no monitor with
    /// this id.
    pub fn remove_monitor(&self, id: u32) {
        self.inner.remove_monitor(MonitorId::from_u32(id));
    }

    /// Gives a monitor another size and work area, as a change of resolution or scale, or a panel
    /// that moves, does. Nothing is done when there is no monitor with this id.
    pub fn resize_monitor(&self, id: u32, size: Rect, work_area: Rect) {
        self.inner
            .resize_monitor(MonitorId::from_u32(id), size, work_area);
    }

    /// Adds a monitor. Its serial number identity is its name.
    pub fn add_monitor(&self, id: u32, name: &str, size: Rect, work_area: Rect) {
        self.inner
            .add_monitor(MonitorId::from_u32(id), name, size, work_area);
    }

    /// Opens a window of an application. It starts at 800x600 at (100, 100), a normal window with
    /// no focus, on the workspace the first monitor shows.
    pub fn open_window(&self, id: u32, exe: &str, class: &str, title: &str) {
        self.inner.open_window_as(wid(id), exe, class, title);
    }

    /// Destroys a window, as the user closing it does: the window manager is not asked. If it had
    /// the focus, nothing has it now.
    pub fn close_window_now(&self, id: u32) {
        self.inner.close_window_now(wid(id));
    }

    /// Marks a window as a dialog, a splash screen, a utility window or the like, or as a normal
    /// one again.
    pub fn set_normal(&self, id: u32, normal: bool) {
        self.inner.set_normal(wid(id), normal);
    }

    /// Puts a window somewhere without the window manager having asked, as a user dragging it
    /// does.
    pub fn move_window_by_user(&self, id: u32, rect: Rect) {
        self.inner.move_window_by_user(wid(id), rect);
    }

    /// Sets what [`Backend::drag_end`] answers for a window: the frame the user asked for when
    /// they let go of one of its edges, as a client that has not finished drawing is short of it.
    /// [`FakeBackend::move_window_by_user`] puts the frame the window has.
    pub fn set_drag_end(&self, id: u32, rect: Rect) {
        self.kde.lock().drag_ends.insert(id, rect);
    }

    /// Gives a window a minimum size. A [`Backend::set_rect`] for something smaller leaves it at
    /// the minimum, which is how a GTK4 client answers.
    pub fn set_min_size(&self, id: u32, width: i32, height: i32) {
        self.inner.set_min_size(wid(id), width, height);
    }

    /// Changes a window's title.
    pub fn set_title(&self, id: u32, title: &str) {
        self.inner.set_title(wid(id), title);
    }

    /// Changes which window has the focus, as the user does, without the window manager asking.
    pub fn set_foreground(&self, id: Option<u32>) {
        self.inner.set_foreground(id.map(wid));
    }

    /// Puts the pointer somewhere.
    pub fn set_cursor(&self, x: i32, y: i32) {
        self.inner.set_cursor(x, y);
    }

    /// Maximizes or restores a window as the user does with its title bar: the compositor does it
    /// and nobody asks the window manager, so no command is recorded. A test sends the event that
    /// says so.
    pub fn user_maximize(&self, id: u32, maximize: bool) {
        self.inner.user_maximize(wid(id), maximize);
    }

    /// Puts a window in or out of fullscreen as a compositor does, without the window manager
    /// being asked, so no command is recorded: the window covers its monitor, and when it leaves
    /// fullscreen it gets the frame it had before, which may not be where the window manager would
    /// put it now. A test sends the event that says so.
    pub fn set_fullscreen(&self, id: u32, fullscreen: bool) {
        self.inner.set_fullscreen(wid(id), fullscreen);
    }

    /// Returns true if and only if the window is maximized, by a command or by
    /// [`FakeBackend::user_maximize`], and has not been restored since.
    pub fn is_maximized(&self, id: u32) -> bool {
        self.inner.is_maximized(wid(id))
    }

    /// Returns where a window is, if it was ever opened.
    pub fn rect_of(&self, id: u32) -> Option<Rect> {
        self.inner.rect_of(wid(id))
    }

    /// Returns true if and only if the window is hidden ([`Backend::hide`]), as opposed to being
    /// on a workspace that is not showing.
    pub fn is_hidden(&self, id: u32) -> bool {
        self.inner.is_hidden(wid(id))
    }

    /// Returns true if and only if the window can be seen: it is alive, not minimized, not hidden
    /// and on the workspace its monitor shows. This is what the compositor does with desktops.
    pub fn on_screen(&self, id: u32) -> bool {
        self.inner.on_screen(wid(id))
    }

    /// Returns the workspace a window is on, if it was ever opened.
    pub fn workspace_of(&self, id: u32) -> Option<usize> {
        self.inner.workspace_of(wid(id))
    }

    /// Returns the workspace a monitor shows.
    pub fn shown_workspace(&self, monitor_id: u32) -> usize {
        self.inner.shown_workspace(MonitorId::from_u32(monitor_id))
    }

    /// Shows workspace `idx` on a monitor the way something other than komorebi does (a pager, a
    /// shortcut, a click on a window of another desktop): no command is recorded, and the
    /// compositor's event about it is the test's to send.
    pub fn show_workspace_by_user(&self, monitor_id: u32, idx: usize) {
        self.inner
            .show_workspace_by_user(MonitorId::from_u32(monitor_id), idx);
    }

    /// Returns how opaque the window manager last asked this window to be, 255 being not see-through
    /// at all, which is what a window is until it is asked otherwise.
    pub fn opacity_of(&self, id: u32) -> u8 {
        self.inner.opacity_of(wid(id))
    }

    /// Returns the border color the window manager last gave this window, `0xRRGGBB`, if it gave
    /// it one.
    pub fn border_colour_of(&self, id: u32) -> Option<u32> {
        self.kde.lock().border_colours.get(&id).copied()
    }

    /// Returns the wallpaper the window manager last asked this monitor to show, if it asked.
    pub fn wallpaper_of(&self, monitor_id: u32) -> Option<PathBuf> {
        self.kde.lock().wallpapers.get(&monitor_id).cloned()
    }

    /// Says whether the compositor has an effect that draws borders. Without one, which is how a
    /// fake starts, [`Backend::apply_look`] answers false however much of a border it is asked
    /// for, as it does on a machine where the effect is not installed.
    pub fn set_borders_drawn(&self, drawn: bool) {
        self.kde.lock().borders_drawn = drawn;
    }

    /// Returns every command received so far, oldest first.
    pub fn commands(&self) -> Vec<Command> {
        self.inner
            .commands()
            .into_iter()
            .filter_map(command)
            .collect()
    }

    /// Returns every command received so far that is not about the look, oldest first.
    ///
    /// The window manager asks for the look, and tells the colors of the borders, after every event
    /// and every message, and the fake records each one that is not what it was told before: a
    /// test that checks what the window manager did to the windows exactly compares this, and a
    /// test about the look compares [`FakeBackend::commands`].
    pub fn window_commands(&self) -> Vec<Command> {
        let mut commands = self.commands();
        commands.retain(|command| {
            !matches!(command, Command::ApplyLook(_) | Command::BorderColours(_))
        });
        commands
    }

    /// Forgets the commands received so far.
    pub fn clear_commands(&self) {
        self.inner.clear_commands();
    }
}

impl Backend for FakeBackend {
    fn windows(&self) -> Vec<WindowInfo> {
        self.inner.windows().into_iter().map(window_info).collect()
    }

    fn window(&self, id: WindowId) -> Option<WindowInfo> {
        self.inner.window(id).map(window_info)
    }

    fn foreground_window(&self) -> Option<WindowId> {
        self.inner.foreground_window()
    }

    fn monitors(&self) -> shared::Result<Vec<MonitorInfo>> {
        Ok(self
            .inner
            .monitors()?
            .into_iter()
            .map(|m| MonitorInfo {
                id: raw_monitor(m.id),
                name: m.name.unwrap_or_default(),
                serial_number_id: m.serial_number_id.unwrap_or_default(),
                size: m.size,
                work_area: m.work_area,
            })
            .collect())
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        self.inner.cursor_pos()
    }

    fn drag_end(&self, id: WindowId) -> Option<Rect> {
        self.kde.lock().drag_ends.get(&raw(id)).copied()
    }

    fn set_rect(&self, id: WindowId, rect: &Rect) -> shared::Result<()> {
        self.inner.set_rect(id, rect)
    }

    fn focus(&self, id: WindowId, mouse_follows_focus: bool) -> shared::Result<()> {
        self.inner.focus(id, mouse_follows_focus)
    }

    fn raise(&self, id: WindowId) -> shared::Result<()> {
        self.inner.raise(id)
    }

    fn hide(&self, id: WindowId) -> shared::Result<()> {
        self.inner.hide(id)
    }

    fn restore(&self, id: WindowId) -> shared::Result<()> {
        self.inner.restore(id)
    }

    fn minimize(&self, id: WindowId) -> shared::Result<()> {
        self.inner.minimize(id)
    }

    fn unminimize(&self, id: WindowId) -> shared::Result<()> {
        self.inner.unminimize(id)
    }

    fn close(&self, id: WindowId) -> shared::Result<()> {
        self.inner.close(id)
    }

    fn center_cursor_in(&self, rect: &Rect) -> shared::Result<()> {
        self.inner.center_cursor_in(rect)
    }

    fn ensure_workspaces(&self, names: &[String]) -> shared::Result<()> {
        self.inner.ensure_workspaces(names)
    }

    fn switch_workspace(&self, monitor_id: MonitorId, idx: usize) -> shared::Result<()> {
        self.inner.switch_workspace(monitor_id, idx)
    }

    fn set_window_workspace(&self, id: WindowId, idx: usize) -> shared::Result<()> {
        self.inner.set_window_workspace(id, idx)
    }

    fn window_workspace(&self, id: WindowId) -> komorebi_backend::Result<Option<usize>> {
        self.inner.window_workspace(id)
    }

    fn current_workspaces(&self) -> shared::Result<Vec<(MonitorId, usize)>> {
        self.inner.current_workspaces()
    }

    fn maximize(&self, id: WindowId, maximize: bool) -> shared::Result<()> {
        self.inner.maximize(id, maximize)
    }

    fn apply_look(&self, look: &Look) -> eyre::Result<bool> {
        let mut kde = self.kde.lock();
        // like a real backend, remember what was applied and say nothing twice
        if kde.last_look.as_ref() != Some(look) {
            self.inner.record_ext(KdeCommand::ApplyLook(look.clone()));
            kde.last_look = Some(look.clone());
        }
        Ok(kde.borders_drawn && look.border.is_some())
    }

    fn set_focus_follows_mouse(&self, enable: bool) -> shared::Result<()> {
        self.inner.set_focus_follows_mouse(enable)
    }

    fn set_wallpaper(&self, monitor_id: MonitorId, path: &Path) -> eyre::Result<()> {
        self.kde
            .lock()
            .wallpapers
            .insert(raw_monitor(monitor_id), path.to_path_buf());
        self.inner.record_ext(KdeCommand::SetWallpaper(
            raw_monitor(monitor_id),
            path.to_path_buf(),
        ));
        Ok(())
    }

    fn set_opacity(&self, id: WindowId, alpha: u8) -> shared::Result<()> {
        self.inner.set_opacity(id, alpha)
    }

    fn set_border_colours(&self, colours: &[(u32, u32)]) -> eyre::Result<bool> {
        let mut kde = self.kde.lock();
        let colours_now: HashMap<u32, u32> = colours.iter().copied().collect();
        // like a real backend: nothing is sent for what has not changed
        if kde.border_colours != colours_now {
            kde.border_colours = colours_now;
            self.inner
                .record_ext(KdeCommand::BorderColours(colours.to_vec()));
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(left: i32, width: i32) -> Rect {
        Rect {
            left,
            top: 0,
            right: width,
            bottom: 1050,
        }
    }

    fn fake_with(ids: &[u32]) -> FakeBackend {
        let fake = FakeBackend::with_one_monitor();
        for id in ids {
            fake.open_window(*id, "foot", "foot", &format!("window {id}"));
        }
        fake
    }

    #[test]
    fn a_window_asked_to_move_is_where_it_was_asked() -> eyre::Result<()> {
        let fake = fake_with(&[1]);

        fake.set_rect(1.into(), &tile(10, 500))?;

        assert_eq!(fake.rect_of(1), Some(tile(10, 500)));
        Ok(())
    }

    #[test]
    fn a_window_keeps_to_its_minimum_size() -> eyre::Result<()> {
        let fake = fake_with(&[1]);
        fake.set_min_size(1, 600, 400);

        fake.set_rect(1.into(), &tile(0, 300))?;

        let rect = fake.rect_of(1).expect("window 1 was opened");
        assert_eq!((rect.right, rect.bottom), (600, 1050));
        Ok(())
    }

    #[test]
    fn every_command_is_recorded_in_order() -> eyre::Result<()> {
        let fake = fake_with(&[1, 2]);

        fake.focus(1.into(), false)?;
        fake.raise(2.into())?;
        fake.minimize(1.into())?;
        fake.unminimize(1.into())?;
        fake.close(2.into())?;

        assert_eq!(
            fake.commands(),
            [
                Command::Focus(1, false),
                Command::Raise(2),
                Command::Minimize(1),
                Command::Unminimize(1),
                Command::Close(2),
            ]
        );
        fake.clear_commands();
        assert!(fake.commands().is_empty());
        Ok(())
    }

    #[test]
    fn a_maximized_window_has_the_work_area_and_gets_its_frame_back() -> eyre::Result<()> {
        let fake = fake_with(&[1]);
        fake.set_rect(1.into(), &tile(100, 200))?;

        fake.maximize(1.into(), true)?;
        // a second maximize does not make the maximized frame the one to give back
        fake.maximize(1.into(), true)?;

        assert!(fake.is_maximized(1));
        assert_eq!(fake.rect_of(1), Some(tile(0, 1920)));

        fake.maximize(1.into(), false)?;

        assert!(!fake.is_maximized(1));
        assert_eq!(fake.rect_of(1), Some(tile(100, 200)));
        assert_eq!(
            fake.commands(),
            [
                Command::SetRect(1, tile(100, 200)),
                Command::Maximize(1, true),
                Command::Maximize(1, true),
                Command::Maximize(1, false),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_window_the_user_maximizes_is_maximized_and_no_command_is_recorded() {
        let fake = fake_with(&[1]);
        fake.set_rect(1.into(), &tile(100, 200)).unwrap();
        fake.clear_commands();

        fake.user_maximize(1, true);

        assert!(fake.is_maximized(1));
        assert!(
            fake.window(1.into())
                .expect("window 1 was opened")
                .maximized
        );
        assert!(fake.windows()[0].maximized);
        assert_eq!(fake.rect_of(1), Some(tile(0, 1920)));
        assert!(fake.commands().is_empty());

        fake.user_maximize(1, false);

        assert!(
            !fake
                .window(1.into())
                .expect("window 1 was opened")
                .maximized
        );
        assert_eq!(fake.rect_of(1), Some(tile(100, 200)));
    }

    #[test]
    fn a_fullscreen_window_covers_its_monitor_and_gets_its_frame_back() {
        let fake = fake_with(&[1]);
        fake.set_rect(1.into(), &tile(100, 200)).unwrap();
        fake.clear_commands();

        fake.set_fullscreen(1, true);

        assert!(
            fake.window(1.into())
                .expect("window 1 was opened")
                .fullscreen
        );
        assert!(fake.windows()[0].fullscreen);
        assert_eq!(
            fake.rect_of(1),
            Some(Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }),
            "the whole monitor, panel included"
        );
        assert!(fake.commands().is_empty());

        fake.set_fullscreen(1, false);

        assert!(
            !fake
                .window(1.into())
                .expect("window 1 was opened")
                .fullscreen
        );
        assert_eq!(fake.rect_of(1), Some(tile(100, 200)));
    }

    #[test]
    fn a_command_about_a_window_nobody_opened_is_an_error() {
        let fake = fake_with(&[1]);

        let error = fake
            .focus(9.into(), false)
            .expect_err("window 9 does not exist");

        assert_eq!(error.to_string(), "no such window: 9");
    }

    #[test]
    fn a_command_about_a_closed_window_is_an_error() {
        let fake = fake_with(&[1]);
        fake.close_window_now(1);

        assert!(fake.raise(1.into()).is_err());
    }

    #[test]
    fn a_permissive_fake_creates_the_windows_it_is_asked_about() -> eyre::Result<()> {
        let fake = FakeBackend::permissive();

        fake.focus(77.into(), false)?;

        assert!(fake.is_window(77.into()));
        assert_eq!(
            fake.foreground_window().and_then(|id| id.to_u32()),
            Some(77)
        );
        Ok(())
    }

    #[test]
    fn focusing_with_the_mouse_following_puts_the_pointer_in_the_middle_of_the_window()
    -> eyre::Result<()> {
        let fake = fake_with(&[1]);
        fake.set_rect(1.into(), &tile(100, 200))?;

        fake.focus(1.into(), true)?;

        assert_eq!(fake.cursor_pos(), Some((200, 525)));
        Ok(())
    }

    #[test]
    fn focusing_without_the_mouse_following_leaves_the_pointer_alone() -> eyre::Result<()> {
        let fake = fake_with(&[1]);
        fake.set_cursor(5, 5);

        fake.focus(1.into(), false)?;

        assert_eq!(fake.cursor_pos(), Some((5, 5)));
        Ok(())
    }

    #[test]
    fn closing_the_window_with_the_focus_leaves_nothing_focused() {
        let fake = fake_with(&[1]);
        fake.set_foreground(Some(1));

        fake.close_window_now(1);

        assert_eq!(fake.foreground_window(), None);
    }

    #[test]
    fn what_the_user_does_is_not_a_command() {
        let fake = fake_with(&[1]);

        fake.move_window_by_user(1, tile(300, 300));
        fake.set_title(1, "renamed");
        fake.set_normal(1, false);

        assert!(fake.commands().is_empty());
        assert_eq!(fake.rect_of(1), Some(tile(300, 300)));
        let window = fake.window(1.into()).expect("window 1 was opened");
        assert_eq!(window.title, "renamed");
        assert!(!window.normal);
    }

    #[test]
    fn windows_are_listed_by_id_and_a_destroyed_one_is_left_out() {
        let fake = fake_with(&[3, 1, 2]);
        fake.close_window_now(2);

        let ids: Vec<_> = fake.windows().iter().map(|w| w.id).collect();

        assert_eq!(ids, [1, 3]);
    }

    #[test]
    fn a_window_opens_on_the_workspace_the_first_monitor_shows() {
        let fake = FakeBackend::with_one_monitor();
        fake.show_workspace_by_user(1, 2);

        fake.open_window(1, "foot", "foot", "window 1");

        assert_eq!(fake.workspace_of(1), Some(2));
        assert_eq!(fake.window_workspace(1.into()).unwrap(), Some(2));
        assert!(fake.on_screen(1));
    }

    #[test]
    fn a_monitor_shows_the_windows_of_its_workspace_and_no_others() -> eyre::Result<()> {
        let fake = fake_with(&[1, 2]);
        fake.set_window_workspace(2.into(), 1)?;

        assert!(fake.on_screen(1) && !fake.on_screen(2));

        fake.switch_workspace(1.into(), 1)?;

        assert!(!fake.on_screen(1) && fake.on_screen(2));
        assert_eq!(fake.shown_workspace(1), 1);
        assert_eq!(
            fake.current_workspaces().unwrap(),
            [(MonitorId::from_u32(1), 1)]
        );
        assert_eq!(
            fake.commands(),
            [
                Command::SetWindowWorkspace(2, 1),
                Command::SwitchWorkspace(1, 1)
            ]
        );
        Ok(())
    }

    #[test]
    fn a_workspace_shown_by_the_user_is_not_a_command() {
        let fake = fake_with(&[1]);

        fake.show_workspace_by_user(1, 3);

        assert_eq!(
            fake.current_workspaces().unwrap(),
            [(MonitorId::from_u32(1), 3)]
        );
        assert!(fake.commands().is_empty());
    }

    #[test]
    fn a_minimized_window_or_one_that_closed_is_not_on_screen() -> eyre::Result<()> {
        let fake = fake_with(&[1, 2]);

        fake.minimize(1.into())?;
        fake.close_window_now(2);

        assert!(!fake.on_screen(1) && !fake.on_screen(2));
        Ok(())
    }

    #[test]
    fn a_hidden_window_is_out_of_sight_on_its_workspace_until_it_is_restored() -> eyre::Result<()> {
        let fake = fake_with(&[1]);

        fake.hide(1.into())?;
        fake.hide(1.into())?;

        assert!(fake.is_hidden(1) && !fake.on_screen(1));
        assert_eq!(fake.window_workspace(1.into()).unwrap(), Some(0));
        assert!(
            fake.window(1.into())
                .is_some_and(|w| w.alive && !w.minimized)
        );

        fake.restore(1.into())?;

        assert!(!fake.is_hidden(1) && fake.on_screen(1));
        assert_eq!(
            fake.commands(),
            [Command::Hide(1), Command::Hide(1), Command::Restore(1)]
        );
        Ok(())
    }

    #[test]
    fn hiding_or_restoring_a_window_nobody_opened_is_an_error() {
        let fake = fake_with(&[1]);

        assert!(fake.hide(9.into()).is_err());
        assert!(fake.restore(9.into()).is_err());
    }

    #[test]
    fn putting_a_window_nobody_opened_on_a_workspace_is_an_error() {
        let fake = fake_with(&[1]);

        assert!(fake.set_window_workspace(9.into(), 1).is_err());
        assert_eq!(fake.workspace_of(9), None);
    }

    #[test]
    fn a_look_is_a_command_once_and_says_whether_borders_are_drawn() -> eyre::Result<()> {
        let fake = FakeBackend::new();
        fake.set_borders_drawn(true);
        let bordered = Look {
            border: Some(crate::backend::BorderLook {
                width: 10,
                radius: 10,
                focused: (1, 2, 3),
                unfocused: (4, 5, 6),
            }),
            animation: None,
        };

        assert!(fake.apply_look(&bordered)?);
        assert!(fake.apply_look(&bordered)?);
        assert!(!fake.apply_look(&Look::default())?);

        fake.set_borders_drawn(false);
        assert!(!fake.apply_look(&bordered)?);

        assert_eq!(
            fake.commands(),
            [
                Command::ApplyLook(bordered.clone()),
                Command::ApplyLook(Look::default()),
                Command::ApplyLook(bordered)
            ]
        );
        Ok(())
    }

    #[test]
    fn border_colours_are_a_command_only_when_they_change() -> eyre::Result<()> {
        let fake = fake_with(&[1, 2]);

        assert!(fake.set_border_colours(&[(1, 0xFF0000), (2, 0x00FF00)])?);
        assert!(fake.set_border_colours(&[(2, 0x00FF00), (1, 0xFF0000)])?);
        assert_eq!(fake.border_colour_of(1), Some(0xFF0000));
        assert_eq!(fake.border_colour_of(2), Some(0x00FF00));

        fake.set_border_colours(&[(2, 0x00FF00)])?;
        assert_eq!(fake.border_colour_of(1), None);
        assert_eq!(
            fake.commands(),
            [
                Command::BorderColours(vec![(1, 0xFF0000), (2, 0x00FF00)]),
                Command::BorderColours(vec![(2, 0x00FF00)]),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_window_is_opaque_until_it_is_asked_to_be_otherwise() -> eyre::Result<()> {
        let fake = fake_with(&[1]);
        assert_eq!(fake.opacity_of(1), 255);

        fake.set_opacity(1.into(), 100)?;

        assert_eq!(fake.opacity_of(1), 100);
        assert_eq!(fake.commands(), [Command::SetOpacity(1, 100)]);
        assert!(fake.set_opacity(9.into(), 100).is_err());
        Ok(())
    }

    #[test]
    fn window_commands_leave_out_the_look() -> eyre::Result<()> {
        let fake = fake_with(&[1]);

        fake.apply_look(&Look::default())?;
        fake.focus(1.into(), false)?;
        fake.set_border_colours(&[(1, 0xFF0000)])?;

        assert_eq!(fake.commands().len(), 3);
        assert_eq!(fake.window_commands(), [Command::Focus(1, false)]);
        Ok(())
    }

    #[test]
    fn the_cursor_can_be_centered_in_a_rectangle() -> eyre::Result<()> {
        let fake = FakeBackend::new();

        fake.center_cursor_in(&tile(0, 400))?;

        assert_eq!(fake.cursor_pos(), Some((200, 525)));
        assert_eq!(fake.commands(), [Command::CenterCursor(tile(0, 400))]);
        Ok(())
    }
}
