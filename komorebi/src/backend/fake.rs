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
//! fake.set_rect(1, &tile)?;
//!
//! assert_eq!(fake.rect_of(1), Some(tile));
//! assert_eq!(fake.commands(), [Command::SetRect(1, tile)]);
//! # Ok::<(), color_eyre::eyre::Report>(())
//! ```

#![deny(missing_docs)]

use super::Backend;
use super::Look;
use super::MonitorInfo;
use super::WindowInfo;
use crate::core::Rect;
use color_eyre::eyre;
use parking_lot::Mutex;
use std::collections::HashMap;

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
}

#[derive(Default)]
struct State {
    /// When set, a command about a window nobody opened creates that window first (see
    /// [`FakeBackend::permissive`]).
    permissive: bool,
    windows: HashMap<u32, WindowInfo>,
    /// the windows that are hidden, in the order they were
    hidden: Vec<u32>,
    min_sizes: HashMap<u32, (i32, i32)>,
    monitors: Vec<MonitorInfo>,
    foreground: Option<u32>,
    cursor: Option<(i32, i32)>,
    commands: Vec<Command>,
    /// windows the compositor has maximized, with the frame to give back
    maximized: HashMap<u32, Rect>,
    /// how many virtual desktops there are
    desktops: usize,
    /// the desktop every window is on
    window_ws: HashMap<u32, usize>,
    /// the desktop each monitor (by id) shows
    shown: HashMap<u32, usize>,
    /// how opaque each window was last asked to be
    opacity: HashMap<u32, u8>,
    /// the border color each window was last given, `0xRRGGBB`
    border_colours: HashMap<u32, u32>,
    /// the look last applied, so that asking for it again is not a command
    last_look: Option<Look>,
    /// whether the compositor has an effect that draws borders
    borders_drawn: bool,
}

impl State {
    /// Maximizes a window to the work area of its monitor, or gives it back the frame it had
    fn set_maximized(&mut self, id: u32, maximize: bool) {
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
                .find(|m| {
                    center.0 >= m.size.left
                        && center.0 < m.size.left + m.size.right
                        && center.1 >= m.size.top
                        && center.1 < m.size.top + m.size.bottom
                })
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
    fn monitor_of(&self, id: u32) -> Option<u32> {
        let w = self.windows.get(&id)?;
        let (x, y) = (
            w.rect.left + w.rect.right / 2,
            w.rect.top + w.rect.bottom / 2,
        );
        self.monitors
            .iter()
            .find(|m| {
                x >= m.size.left
                    && x < m.size.left + m.size.right
                    && y >= m.size.top
                    && y < m.size.top + m.size.bottom
            })
            .or_else(|| self.monitors.first())
            .map(|m| m.id)
    }

    /// What a window opened now would be on, as in KWin: the desktop its monitor is showing
    fn desktop_for_new_window(&self) -> usize {
        self.monitors
            .first()
            .and_then(|m| self.shown.get(&m.id))
            .copied()
            .unwrap_or(0)
    }
}

/// An in-memory [`Backend`].
///
/// It has no windows and no monitors until a test adds them. The window manager reaches it through
/// [`crate::backend::backend`], installed with [`crate::backend::with_backend`].
#[derive(Default)]
pub struct FakeBackend {
    state: Mutex<State>,
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
        let fake = Self::with_one_monitor();
        fake.state.lock().permissive = true;
        fake
    }

    /// Returns a fake with one 1920x1080 monitor, named `Virtual-0`, whose work area leaves 30
    /// pixels at the bottom for a panel.
    pub fn with_one_monitor() -> Self {
        let fake = Self::new();
        fake.add_monitor(
            1,
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

    /// Adds a monitor. Its serial number identity is its name.
    pub fn add_monitor(&self, id: u32, name: &str, size: Rect, work_area: Rect) {
        self.state.lock().monitors.push(MonitorInfo {
            id,
            name: name.to_string(),
            serial_number_id: name.to_string(),
            size,
            work_area,
        });
    }

    /// Opens a window of an application. It starts at 800x600 at (100, 100), a normal window with
    /// no focus, on the workspace the first monitor shows.
    pub fn open_window(&self, id: u32, exe: &str, class: &str, title: &str) {
        let mut state = self.state.lock();
        let desktop = state.desktop_for_new_window();
        state.window_ws.insert(id, desktop);
        state.windows.insert(
            id,
            WindowInfo {
                id,
                pid: i32::try_from(id).unwrap_or_default() + 1000,
                title: title.to_string(),
                class: class.to_string(),
                exe: exe.to_string(),
                path: format!("/usr/bin/{exe}"),
                rect: Rect {
                    left: 100,
                    top: 100,
                    right: 800,
                    bottom: 600,
                },
                minimized: false,
                normal: true,
                alive: true,
            },
        );
    }

    /// Destroys a window, as the user closing it does: the window manager is not asked. If it had
    /// the focus, nothing has it now.
    pub fn close_window_now(&self, id: u32) {
        let mut state = self.state.lock();
        if let Some(window) = state.windows.get_mut(&id) {
            window.alive = false;
        }
        if state.foreground == Some(id) {
            state.foreground = None;
        }
    }

    /// Marks a window as a dialog, a splash screen, a utility window or the like, or as a normal
    /// one again.
    pub fn set_normal(&self, id: u32, normal: bool) {
        if let Some(w) = self.state.lock().windows.get_mut(&id) {
            w.normal = normal;
        }
    }

    /// Puts a window somewhere without the window manager having asked, as a user dragging it
    /// does.
    pub fn move_window_by_user(&self, id: u32, rect: Rect) {
        if let Some(w) = self.state.lock().windows.get_mut(&id) {
            w.rect = rect;
        }
    }

    /// Gives a window a minimum size. A [`Backend::set_rect`] for something smaller leaves it at
    /// the minimum, which is how a GTK4 client answers.
    pub fn set_min_size(&self, id: u32, width: i32, height: i32) {
        self.state.lock().min_sizes.insert(id, (width, height));
    }

    /// Changes a window's title.
    pub fn set_title(&self, id: u32, title: &str) {
        if let Some(w) = self.state.lock().windows.get_mut(&id) {
            w.title = title.to_string();
        }
    }

    /// Changes which window has the focus, as the user does, without the window manager asking.
    pub fn set_foreground(&self, id: Option<u32>) {
        self.state.lock().foreground = id;
    }

    /// Puts the pointer somewhere.
    pub fn set_cursor(&self, x: i32, y: i32) {
        self.state.lock().cursor = Some((x, y));
    }

    /// Returns true if and only if the window has been maximized by a command and not restored
    /// since.
    pub fn is_maximized(&self, id: u32) -> bool {
        self.state.lock().maximized.contains_key(&id)
    }

    /// Returns where a window is, if it was ever opened.
    pub fn rect_of(&self, id: u32) -> Option<Rect> {
        self.state.lock().windows.get(&id).map(|w| w.rect)
    }

    /// Returns true if and only if the window is hidden ([`Backend::hide`]), as opposed to being
    /// on a workspace that is not showing.
    pub fn is_hidden(&self, id: u32) -> bool {
        self.state.lock().hidden.contains(&id)
    }

    /// Returns true if and only if the window can be seen: it is alive, not minimized, not hidden
    /// and on the workspace its monitor shows. This is what the compositor does with desktops.
    pub fn on_screen(&self, id: u32) -> bool {
        let state = self.state.lock();
        let Some(window) = state.windows.get(&id) else {
            return false;
        };
        let monitor = state.monitor_of(id);
        let shown = monitor
            .and_then(|m| state.shown.get(&m))
            .copied()
            .unwrap_or(0);
        window.alive
            && !window.minimized
            && !state.hidden.contains(&id)
            && state.window_ws.get(&id).copied().unwrap_or(0) == shown
    }

    /// Returns the workspace a window is on, if it was ever opened.
    pub fn workspace_of(&self, id: u32) -> Option<usize> {
        self.state.lock().window_ws.get(&id).copied()
    }

    /// Returns the workspace a monitor shows.
    pub fn shown_workspace(&self, monitor_id: u32) -> usize {
        self.state
            .lock()
            .shown
            .get(&monitor_id)
            .copied()
            .unwrap_or(0)
    }

    /// Shows workspace `idx` on a monitor the way something other than komorebi does (a pager, a
    /// shortcut, a click on a window of another desktop): no command is recorded, and the
    /// compositor's event about it is the test's to send.
    pub fn show_workspace_by_user(&self, monitor_id: u32, idx: usize) {
        let mut state = self.state.lock();
        state.desktops = state.desktops.max(idx + 1);
        state.shown.insert(monitor_id, idx);
    }

    /// Returns how opaque the window manager last asked this window to be, 255 being not see-through
    /// at all, which is what a window is until it is asked otherwise.
    pub fn opacity_of(&self, id: u32) -> u8 {
        self.state.lock().opacity.get(&id).copied().unwrap_or(255)
    }

    /// Returns the border color the window manager last gave this window, `0xRRGGBB`, if it gave
    /// it one.
    pub fn border_colour_of(&self, id: u32) -> Option<u32> {
        self.state.lock().border_colours.get(&id).copied()
    }

    /// Says whether the compositor has an effect that draws borders. Without one, which is how a
    /// fake starts, [`Backend::apply_look`] answers false however much of a border it is asked
    /// for, as it does on a machine where the effect is not installed.
    pub fn set_borders_drawn(&self, drawn: bool) {
        self.state.lock().borders_drawn = drawn;
    }

    /// Returns every command received so far, oldest first.
    pub fn commands(&self) -> Vec<Command> {
        self.state.lock().commands.clone()
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
        self.state.lock().commands.clear();
    }

    fn record<T>(
        &self,
        id: u32,
        command: Command,
        f: impl FnOnce(&mut State) -> T,
    ) -> eyre::Result<T> {
        let mut state = self.state.lock();
        state.commands.push(command);
        if state.permissive && !state.windows.contains_key(&id) {
            state.windows.insert(id, permissive_window(id));
        }
        match state.windows.get(&id) {
            Some(w) if w.alive => Ok(f(&mut state)),
            _ => eyre::bail!("no such window: {id}"),
        }
    }
}

fn permissive_window(id: u32) -> WindowInfo {
    WindowInfo {
        id,
        pid: i32::try_from(id).unwrap_or_default() + 1000,
        title: format!("window {id}"),
        class: format!("class.{id}"),
        exe: format!("exe{id}"),
        path: format!("/usr/bin/exe{id}"),
        rect: Rect::default(),
        minimized: false,
        normal: true,
        alive: true,
    }
}

impl Backend for FakeBackend {
    fn windows(&self) -> Vec<WindowInfo> {
        let state = self.state.lock();
        let mut windows: Vec<_> = state
            .windows
            .values()
            .filter(|w| w.alive)
            .cloned()
            .collect();
        windows.sort_by_key(|w| w.id);
        windows
    }

    fn window(&self, id: u32) -> Option<WindowInfo> {
        let mut state = self.state.lock();
        if state.permissive && !state.windows.contains_key(&id) {
            state.windows.insert(id, permissive_window(id));
        }
        state.windows.get(&id).cloned()
    }

    fn foreground_window(&self) -> Option<u32> {
        self.state.lock().foreground
    }

    fn monitors(&self) -> eyre::Result<Vec<MonitorInfo>> {
        Ok(self.state.lock().monitors.clone())
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        self.state.lock().cursor
    }

    fn set_rect(&self, id: u32, rect: &Rect) -> eyre::Result<()> {
        let rect = *rect;
        self.record(id, Command::SetRect(id, rect), |state| {
            let (min_w, min_h) = state.min_sizes.get(&id).copied().unwrap_or((0, 0));
            if let Some(w) = state.windows.get_mut(&id) {
                w.rect = Rect {
                    left: rect.left,
                    top: rect.top,
                    right: rect.right.max(min_w),
                    bottom: rect.bottom.max(min_h),
                };
            }
        })
    }

    fn focus(&self, id: u32, mouse_follows_focus: bool) -> eyre::Result<()> {
        self.record(id, Command::Focus(id, mouse_follows_focus), |state| {
            state.foreground = Some(id);
            if mouse_follows_focus && let Some(w) = state.windows.get(&id) {
                state.cursor = Some((
                    w.rect.left + w.rect.right / 2,
                    w.rect.top + w.rect.bottom / 2,
                ));
            }
        })
    }

    fn raise(&self, id: u32) -> eyre::Result<()> {
        self.record(id, Command::Raise(id), |_| ())
    }

    fn hide(&self, id: u32) -> eyre::Result<()> {
        self.record(id, Command::Hide(id), |state| {
            if !state.hidden.contains(&id) {
                state.hidden.push(id);
            }
        })
    }

    fn restore(&self, id: u32) -> eyre::Result<()> {
        self.record(id, Command::Restore(id), |state| {
            state.hidden.retain(|h| *h != id);
        })
    }

    fn minimize(&self, id: u32) -> eyre::Result<()> {
        self.record(id, Command::Minimize(id), |state| {
            if let Some(w) = state.windows.get_mut(&id) {
                w.minimized = true;
            }
        })
    }

    fn unminimize(&self, id: u32) -> eyre::Result<()> {
        self.record(id, Command::Unminimize(id), |state| {
            if let Some(w) = state.windows.get_mut(&id) {
                w.minimized = false;
            }
        })
    }

    fn close(&self, id: u32) -> eyre::Result<()> {
        self.record(id, Command::Close(id), |state| {
            if let Some(w) = state.windows.get_mut(&id) {
                w.alive = false;
            }
        })
    }

    fn center_cursor_in(&self, rect: &Rect) -> eyre::Result<()> {
        let mut state = self.state.lock();
        state.commands.push(Command::CenterCursor(*rect));
        state.cursor = Some((rect.left + rect.right / 2, rect.top + rect.bottom / 2));
        Ok(())
    }

    fn ensure_workspaces(&self, names: &[String]) -> eyre::Result<()> {
        let mut state = self.state.lock();
        state.commands.push(Command::EnsureWorkspaces(names.len()));
        state.desktops = state.desktops.max(names.len());
        Ok(())
    }

    fn switch_workspace(&self, monitor_id: u32, idx: usize) -> eyre::Result<()> {
        let mut state = self.state.lock();
        state
            .commands
            .push(Command::SwitchWorkspace(monitor_id, idx));
        state.desktops = state.desktops.max(idx + 1);
        state.shown.insert(monitor_id, idx);
        Ok(())
    }

    fn set_window_workspace(&self, id: u32, idx: usize) -> eyre::Result<()> {
        self.record(id, Command::SetWindowWorkspace(id, idx), |state| {
            state.desktops = state.desktops.max(idx + 1);
            state.window_ws.insert(id, idx);
        })
    }

    fn window_workspace(&self, id: u32) -> Option<usize> {
        let mut state = self.state.lock();
        if state.permissive && !state.windows.contains_key(&id) {
            state.windows.insert(id, permissive_window(id));
        }
        state.window_ws.get(&id).copied()
    }

    fn maximize(&self, id: u32, maximize: bool) -> eyre::Result<()> {
        self.record(id, Command::Maximize(id, maximize), |state| {
            state.set_maximized(id, maximize);
        })
    }

    fn current_workspaces(&self) -> Vec<(u32, usize)> {
        let state = self.state.lock();
        state
            .monitors
            .iter()
            .map(|m| (m.id, state.shown.get(&m.id).copied().unwrap_or(0)))
            .collect()
    }

    fn apply_look(&self, look: &Look) -> eyre::Result<bool> {
        let mut state = self.state.lock();
        // like a real backend, remember what was applied and say nothing twice
        if state.last_look.as_ref() != Some(look) {
            state.commands.push(Command::ApplyLook(look.clone()));
            state.last_look = Some(look.clone());
        }
        Ok(state.borders_drawn && look.border.is_some())
    }

    fn set_opacity(&self, id: u32, alpha: u8) -> eyre::Result<()> {
        self.record(id, Command::SetOpacity(id, alpha), |state| {
            state.opacity.insert(id, alpha);
        })
    }

    fn set_border_colours(&self, colours: &[(u32, u32)]) -> eyre::Result<bool> {
        let mut state = self.state.lock();
        let colours_now: HashMap<u32, u32> = colours.iter().copied().collect();
        // like a real backend: nothing is sent for what has not changed
        if state.border_colours != colours_now {
            state.border_colours = colours_now;
            state
                .commands
                .push(Command::BorderColours(colours.to_vec()));
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

        fake.set_rect(1, &tile(10, 500))?;

        assert_eq!(fake.rect_of(1), Some(tile(10, 500)));
        Ok(())
    }

    #[test]
    fn a_window_keeps_to_its_minimum_size() -> eyre::Result<()> {
        let fake = fake_with(&[1]);
        fake.set_min_size(1, 600, 400);

        fake.set_rect(1, &tile(0, 300))?;

        let rect = fake.rect_of(1).expect("window 1 was opened");
        assert_eq!((rect.right, rect.bottom), (600, 1050));
        Ok(())
    }

    #[test]
    fn every_command_is_recorded_in_order() -> eyre::Result<()> {
        let fake = fake_with(&[1, 2]);

        fake.focus(1, false)?;
        fake.raise(2)?;
        fake.minimize(1)?;
        fake.unminimize(1)?;
        fake.close(2)?;

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
        fake.set_rect(1, &tile(100, 200))?;

        fake.maximize(1, true)?;
        // a second maximize does not make the maximized frame the one to give back
        fake.maximize(1, true)?;

        assert!(fake.is_maximized(1));
        assert_eq!(fake.rect_of(1), Some(tile(0, 1920)));

        fake.maximize(1, false)?;

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
    fn a_command_about_a_window_nobody_opened_is_an_error() {
        let fake = fake_with(&[1]);

        let error = fake.focus(9, false).expect_err("window 9 does not exist");

        assert_eq!(error.to_string(), "no such window: 9");
    }

    #[test]
    fn a_command_about_a_closed_window_is_an_error() {
        let fake = fake_with(&[1]);
        fake.close_window_now(1);

        assert!(fake.raise(1).is_err());
    }

    #[test]
    fn a_permissive_fake_creates_the_windows_it_is_asked_about() -> eyre::Result<()> {
        let fake = FakeBackend::permissive();

        fake.focus(77, false)?;

        assert!(fake.is_window(77));
        assert_eq!(fake.foreground_window(), Some(77));
        Ok(())
    }

    #[test]
    fn focusing_with_the_mouse_following_puts_the_pointer_in_the_middle_of_the_window()
    -> eyre::Result<()> {
        let fake = fake_with(&[1]);
        fake.set_rect(1, &tile(100, 200))?;

        fake.focus(1, true)?;

        assert_eq!(fake.cursor_pos(), Some((200, 525)));
        Ok(())
    }

    #[test]
    fn focusing_without_the_mouse_following_leaves_the_pointer_alone() -> eyre::Result<()> {
        let fake = fake_with(&[1]);
        fake.set_cursor(5, 5);

        fake.focus(1, false)?;

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
        let window = fake.window(1).expect("window 1 was opened");
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
        assert_eq!(fake.window_workspace(1), Some(2));
        assert!(fake.on_screen(1));
    }

    #[test]
    fn a_monitor_shows_the_windows_of_its_workspace_and_no_others() -> eyre::Result<()> {
        let fake = fake_with(&[1, 2]);
        fake.set_window_workspace(2, 1)?;

        assert!(fake.on_screen(1) && !fake.on_screen(2));

        fake.switch_workspace(1, 1)?;

        assert!(!fake.on_screen(1) && fake.on_screen(2));
        assert_eq!(fake.shown_workspace(1), 1);
        assert_eq!(fake.current_workspaces(), [(1, 1)]);
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

        assert_eq!(fake.current_workspaces(), [(1, 3)]);
        assert!(fake.commands().is_empty());
    }

    #[test]
    fn a_minimized_window_or_one_that_closed_is_not_on_screen() -> eyre::Result<()> {
        let fake = fake_with(&[1, 2]);

        fake.minimize(1)?;
        fake.close_window_now(2);

        assert!(!fake.on_screen(1) && !fake.on_screen(2));
        Ok(())
    }

    #[test]
    fn a_hidden_window_is_out_of_sight_on_its_workspace_until_it_is_restored() -> eyre::Result<()> {
        let fake = fake_with(&[1]);

        fake.hide(1)?;
        fake.hide(1)?;

        assert!(fake.is_hidden(1) && !fake.on_screen(1));
        assert_eq!(fake.window_workspace(1), Some(0));
        assert!(fake.window(1).is_some_and(|w| w.alive && !w.minimized));

        fake.restore(1)?;

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

        assert!(fake.hide(9).is_err());
        assert!(fake.restore(9).is_err());
    }

    #[test]
    fn putting_a_window_nobody_opened_on_a_workspace_is_an_error() {
        let fake = fake_with(&[1]);

        assert!(fake.set_window_workspace(9, 1).is_err());
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

        fake.set_opacity(1, 100)?;

        assert_eq!(fake.opacity_of(1), 100);
        assert_eq!(fake.commands(), [Command::SetOpacity(1, 100)]);
        assert!(fake.set_opacity(9, 100).is_err());
        Ok(())
    }

    #[test]
    fn window_commands_leave_out_the_look() -> eyre::Result<()> {
        let fake = fake_with(&[1]);

        fake.apply_look(&Look::default())?;
        fake.focus(1, false)?;
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
