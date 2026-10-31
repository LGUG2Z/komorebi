use crate::id::MonitorId;
use crate::id::WindowId;
use komorebi_layouts::Rect;

/// What a backend knows about one window: plain data, a snapshot.
///
/// Only `id`, `pid`, `rect`, `minimized` and `alive` are available on every platform cheaply. The
/// rest is `None` where the platform has no source for it or would have to ask the application
/// (`docs/dev/seam.md` finding 6): macOS has no window class and no `fullscreen`, `maximized` or
/// `normal`, and reads each string from the application over Accessibility; Windows asks the
/// system once per field. KDE (the only platform that delivers all of them in one snapshot)
/// fills every field.
///
/// `rect` is the frame as the platform defines it (see [`crate::Backend::window_rect`]), in the
/// shared [`Rect`] convention: `right` and `bottom` hold the width and height.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowInfo {
    /// The window's id.
    pub id: WindowId,
    /// The id of the process that owns the window.
    pub pid: i32,
    /// The window's frame.
    pub rect: Rect,
    /// Whether the window is minimized.
    pub minimized: bool,
    /// False once the window has been destroyed. A backend may keep a destroyed window
    /// queryable for a while (KDE does), with this false.
    pub alive: bool,
    /// The window's title.
    pub title: Option<String>,
    /// The window class: the Wayland `app_id` on KDE, the Win32 class on Windows, nothing on
    /// macOS.
    pub class: Option<String>,
    /// The file name of the executable.
    pub exe: Option<String>,
    /// The full path of the executable.
    pub path: Option<String>,
    /// Whether the window covers its monitor, panels included (KDE, Windows).
    pub fullscreen: Option<bool>,
    /// Whether the window is maximized (KDE, Windows).
    pub maximized: Option<bool>,
    /// Whether the platform considers this an ordinary application window as opposed to a dialog,
    /// a splash screen, a dock and so on (KDE).
    pub normal: Option<bool>,
}

/// A connected monitor, in the pixels the platform lays windows out in (logical on KDE, physical
/// on Windows, points on macOS: `docs/dev/seam.md` section 5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonitorInfo {
    /// The monitor's id.
    pub id: MonitorId,
    /// The bounds of the monitor.
    pub size: Rect,
    /// `size` minus the panels, docks and menu bars the platform reserves.
    pub work_area: Rect,
    /// The name of the output (`DP-1`), where the platform has one.
    pub name: Option<String>,
    /// An identity that stays the same across a hot-plug and a restart, where the platform has
    /// one.
    pub serial_number_id: Option<String>,
}
