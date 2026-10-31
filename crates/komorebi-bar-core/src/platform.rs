//! What the widgets need from the operating system and what differs between the bars.
//!
//! A shell implements [`BarPlatform`] and calls [`install`] first thing in `main`; the widgets
//! reach it through [`platform`] when they are built or clicked.

use crate::widgets::icons::ImageIcon;
use std::io;
use std::sync::OnceLock;

/// A tool the bar opens when a widget is clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemTool {
    /// The task manager (Task Manager, Activity Monitor)
    TaskManager,
    /// The operating system's power and battery settings
    PowerSettings,
    /// The operating system's network settings
    NetworkSettings,
    /// A URL, in the default browser
    Url(String),
    /// The location a storage widget label names (the label text, as shown: the platform
    /// knows how its labels are built), in the file manager
    StorageLocation(String),
}

pub trait BarPlatform: Send + Sync {
    /// Opens a tool; the result is whether it could be started.
    fn open_system_tool(&self, tool: SystemTool) -> io::Result<()>;

    /// The GitHub API URL that says which release of this platform's komorebi is the latest.
    fn latest_release_api_url(&self) -> &'static str;

    /// The icon of the program a command string would start (Windows: the executable's icon;
    /// macOS: none).
    fn command_icon(&self, command: &str) -> Option<ImageIcon>;

    /// The glyph the date widget shows as its calendar icon.
    fn calendar_icon(&self) -> &'static str;
}

static PLATFORM: OnceLock<&'static dyn BarPlatform> = OnceLock::new();

/// Sets the platform of the process. The first call wins; a second one is ignored.
pub fn install(platform: &'static dyn BarPlatform) {
    let _ = PLATFORM.set(platform);
}

/// # Panics
///
/// Panics if [`install`] has not been called: a shell must install its platform before it builds
/// a widget.
pub fn platform() -> &'static dyn BarPlatform {
    *PLATFORM
        .get()
        .expect("komorebi_bar_core::platform::install must be called first thing in main")
}
