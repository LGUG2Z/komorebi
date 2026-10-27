//! [`Window`], the window manager's handle to a window on the desktop.
//!
//! A window is a `Copy` value that carries only the compositor's id for it. Everything else about
//! the window (its title, its class, where it is) is asked of the
//! [`Backend`](crate::backend::Backend) when it is needed, and everything done to it (a move, the
//! focus, a close) is a command to the backend. Nothing is cached, so a window that changed its
//! title, or closed, is seen as it is now.

use crate::backend::backend;
use crate::core::Rect;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use serde::ser::SerializeStruct;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Write;
use std::path::PathBuf;

/// A handle to a window on the desktop: just its id.
///
/// It is `Copy`, events carry it by value, and everything else (title, class, geometry, ...) is
/// asked of the [`crate::backend::Backend`] when needed. The id is the compositor's
/// ([`WindowInfo::id`](crate::backend::WindowInfo::id)), and stays the same for the life of the
/// window.
#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
pub struct Window {
    pub id: u32,
}

impl From<u32> for Window {
    fn from(id: u32) -> Self {
        Self { id }
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowDetails {
    pub title: String,
    pub exe: String,
    pub class: String,
    pub path: String,
}

impl From<&Window> for WindowDetails {
    fn from(value: &Window) -> Self {
        Self::from(*value)
    }
}

impl From<Window> for WindowDetails {
    fn from(value: Window) -> Self {
        Self {
            title: value.title().unwrap_or_default(),
            exe: value.exe().unwrap_or_default(),
            class: value.class().unwrap_or_default(),
            path: value
                .path()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        }
    }
}

impl Display for Window {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut display = format!("(window_id: {}", self.id);

        if let Some(info) = self.info() {
            write!(display, ", process_id: {}", info.pid)?;
            write!(display, ", title: {}", info.title)?;
            write!(display, ", exe: {}", info.exe)?;
            write!(display, ", class: {}", info.class)?;
        }

        write!(display, ")")?;

        write!(f, "{display}")
    }
}

impl Serialize for Window {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Window", 3)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("rect", &self.rect().unwrap_or_default())?;
        state.serialize_field("details", &WindowDetails::from(*self))?;
        state.end()
    }
}

#[cfg(feature = "schemars")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
struct SerializedWindow {
    id: u32,
    title: String,
    exe: String,
    class: String,
    path: String,
    rect: Rect,
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Window {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Window")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        <SerializedWindow as schemars::JsonSchema>::json_schema(generator)
    }
}

impl Window {
    /// What the compositor knows about this window, including for a few moments after it closed
    pub fn info(self) -> Option<crate::backend::WindowInfo> {
        backend().window(self.id)
    }

    /// Whether the window still exists
    pub fn is_valid(self) -> bool {
        backend().is_window(self.id)
    }

    pub fn is_focused(self) -> bool {
        backend().foreground_window() == Some(self.id)
    }

    pub fn is_minimized(self) -> bool {
        self.info().is_some_and(|i| i.minimized)
    }

    pub fn title(self) -> Option<String> {
        self.info().map(|i| i.title)
    }

    pub fn exe(self) -> Option<String> {
        self.info().map(|i| i.exe)
    }

    /// The Wayland `app_id` (or X11 `WM_CLASS`)
    pub fn class(self) -> Option<String> {
        self.info().map(|i| i.class)
    }

    pub fn path(self) -> Option<PathBuf> {
        self.info().map(|i| PathBuf::from(i.path))
    }

    pub fn process_id(self) -> i32 {
        self.info().map_or(0, |i| i.pid)
    }

    /// The window's current frame geometry, as last reported by the compositor
    pub fn rect(self) -> eyre::Result<Rect> {
        backend()
            .window_rect(self.id)
            .ok_or_eyre("the window does not exist")
    }

    /// Asks the compositor to move and resize the window to `rect`; where the window ends up is
    /// [`Window::rect`], since a client may refuse a size
    pub fn set_position(self, rect: &Rect) -> eyre::Result<()> {
        backend().set_rect(self.id, rect)
    }

    pub fn focus(self, mouse_follows_focus: bool) -> eyre::Result<()> {
        backend().focus(self.id, mouse_follows_focus)
    }

    pub fn raise(self) -> eyre::Result<()> {
        backend().raise(self.id)
    }

    pub fn close(self) -> eyre::Result<()> {
        backend().close(self.id)
    }

    pub fn minimize(self) -> eyre::Result<()> {
        backend().minimize(self.id)
    }

    pub fn unminimize(self) -> eyre::Result<()> {
        backend().unminimize(self.id)
    }

    /// Moves the window to the middle of `work_area`, keeping its size
    ///
    /// # Errors
    ///
    /// The backend does not know the window, or refuses to move it.
    pub fn center(&mut self, work_area: &Rect) -> eyre::Result<()> {
        let (target_width, target_height) = {
            let current_rect = self.rect()?;
            (current_rect.right, current_rect.bottom)
        };

        let x = work_area.left + ((work_area.right - target_width) / 2);
        let y = work_area.top + ((work_area.bottom - target_height) / 2);

        self.set_position(&Rect {
            left: x,
            top: y,
            right: target_width,
            bottom: target_height,
        })
    }

    /// Whether the window is one komorebi should tile: a window that is still alive and that the
    /// compositor considers a normal application window, as opposed to a dialog, a splash screen,
    /// a utility window or a dock ([`WindowInfo::normal`](crate::backend::WindowInfo::normal)).
    ///
    /// A window the compositor has no record of is not managed. Windows that are minimized when
    /// komorebi starts are not picked up either
    /// ([`load_workspace_information`](crate::desktop::load_workspace_information)).
    pub fn should_manage(&self) -> bool {
        let Some(info) = self.info() else {
            return false;
        };

        if !info.alive {
            return false;
        }

        // dialogs, splash screens, utility windows, docks and the like are not for tiling
        info.normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::Command;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use std::sync::Arc;

    fn fake_with(id: u32) -> Arc<FakeBackend> {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(id, "foot", "foot", "a terminal");
        fake
    }

    #[test]
    fn a_window_is_what_the_backend_says_it_is() -> eyre::Result<()> {
        let fake = fake_with(201);
        fake.set_foreground(Some(201));

        with_backend(fake, || {
            let window = Window::from(201);

            assert!(window.is_valid());
            assert!(window.is_focused());
            assert!(!window.is_minimized());
            assert_eq!(window.title().as_deref(), Some("a terminal"));
            assert_eq!(window.exe().as_deref(), Some("foot"));
            assert_eq!(window.class().as_deref(), Some("foot"));
            assert_eq!(window.path(), Some(PathBuf::from("/usr/bin/foot")));
            assert_eq!(window.process_id(), 1201);
            assert_eq!(
                window.rect()?,
                Rect {
                    left: 100,
                    top: 100,
                    right: 800,
                    bottom: 600,
                }
            );
            assert_eq!(
                window.to_string(),
                "(window_id: 201, process_id: 1201, title: a terminal, exe: foot, class: foot)"
            );
            Ok(())
        })
    }

    #[test]
    fn a_window_the_backend_does_not_know_has_no_details() {
        let fake = Arc::new(FakeBackend::with_one_monitor());

        with_backend(fake, || {
            let window = Window::from(202);

            assert!(!window.is_valid());
            assert_eq!(window.title(), None);
            assert_eq!(window.process_id(), 0);
            assert!(window.rect().is_err());
            assert_eq!(window.to_string(), "(window_id: 202)");
        });
    }

    #[test]
    fn every_change_to_a_window_is_a_command_to_the_backend() -> eyre::Result<()> {
        let fake = fake_with(203);
        let tile = Rect {
            left: 0,
            top: 0,
            right: 960,
            bottom: 1050,
        };

        with_backend(fake.clone(), || {
            let window = Window::from(203);

            window.set_position(&tile)?;
            window.focus(false)?;
            window.raise()?;
            window.minimize()?;
            window.unminimize()?;
            window.close()
        })?;

        assert_eq!(
            fake.commands(),
            [
                Command::SetRect(203, tile),
                Command::Focus(203, false),
                Command::Raise(203),
                Command::Minimize(203),
                Command::Unminimize(203),
                Command::Close(203),
            ]
        );
        Ok(())
    }

    #[test]
    fn only_a_normal_window_that_is_alive_is_managed() {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(205, "foot", "foot", "a terminal");
        fake.open_window(206, "foot", "foot", "a dialog");
        fake.set_normal(206, false);
        fake.open_window(207, "foot", "foot", "closed");
        fake.close_window_now(207);

        with_backend(fake, || {
            assert!(Window::from(205).should_manage());
            assert!(!Window::from(206).should_manage());
            assert!(!Window::from(207).should_manage());
            assert!(!Window::from(208).should_manage());
        });
    }

    #[test]
    fn a_window_serializes_with_what_the_backend_knows_about_it() -> eyre::Result<()> {
        let fake = fake_with(204);

        let json = with_backend(fake, || serde_json::to_value(Window::from(204)))?;

        assert_eq!(json["id"], 204);
        assert_eq!(json["rect"]["right"], 800);
        assert_eq!(json["details"]["title"], "a terminal");
        assert_eq!(json["details"]["path"], "/usr/bin/foot");
        Ok(())
    }
}
