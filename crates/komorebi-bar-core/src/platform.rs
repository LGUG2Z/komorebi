//! What the widgets need from the operating system and what differs between the bars.
//!
//! A shell implements [`BarPlatform`] and calls [`install`] first thing in `main`; the widgets
//! reach it through [`platform`] when they are built or clicked.

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
}

pub trait BarPlatform: Send + Sync {
    /// Opens a tool; the result is whether it could be started.
    fn open_system_tool(&self, tool: SystemTool) -> io::Result<()>;

    /// The GitHub API URL that says which release of this platform's komorebi is the latest.
    fn latest_release_api_url(&self) -> &'static str;

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
