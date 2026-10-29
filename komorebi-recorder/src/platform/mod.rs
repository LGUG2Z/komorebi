//! Everything that differs by the OS the recorder runs on.
//!
//! The rest of the crate never names a system API. It asks this module for the handful of things
//! that are not portable, and each host provides them:
//!
//! | module | what it answers |
//! |---|---|
//! | [`clock`] | the master monotonic clock and its tick size |
//! | [`shell`] | the OS version, and (for the recorder window) showing a folder and asking for one |
//! | [`apps`] | which process owns a window id |
//! | [`process`] | stopping a child process |
//! | [`trace`] | streaming the OS trace into the session |
//! | [`capture`] | can we capture, which displays exist, the real [`CaptureBackend`], a video's duration |
//! | [`power`] | hearing about sleep, wake, lock and display changes as they happen |
//! | [`session`] | is the screen locked, is this user's session in front |
//! | [`window`] | what the program's own windows do about being recorded and being tiled |
//!
//! A third host, `unsupported`, builds on anything else so that the investigation commands (which
//! only read a session directory) still work on a Linux agent box; recording there says so.
//!
//! # The host axis
//!
//! This is the *host* axis: what the recorder is running on.
//! Which komorebi produced a session is a separate axis (see [`crate::analysis::flavor`]): a
//! session recorded on Windows can be investigated on a Mac, and the [`Host`] a session was
//! recorded on is part of its manifest.
//!
//! # Adding a function
//!
//! A function that the rest of the crate calls as `platform::<module>::<name>` has to exist, with
//! the same signature, in `macos/<module>.rs`, `win/<module>.rs` and `unsupported.rs`. The
//! compiler enforces that for the host being built, and for no other: the Windows host is built
//! from a Mac by `just check-windows` and by CI, and `unsupported` is built only on a third OS,
//! where nothing in CI builds it.
//!
//! [`CaptureBackend`]: crate::capture::CaptureBackend

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod unsupported;
#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
use unsupported as imp;

// not `windows`: that would be ambiguous with the `windows` crate in every `use` of this module
#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as imp;

pub use imp::apps;
pub use imp::capture;
pub use imp::clock;
#[cfg(windows)]
pub use imp::media;
pub use imp::power;
pub use imp::process;
pub use imp::session;
pub use imp::shell;
pub use imp::trace;
pub mod window;

/// Whether this host has an OS trace to record alongside komorebi's own output.
///
/// That is the unified log on macOS, and the `WinEvents` komorebi hears and the Event Log on
/// Windows. A recording made on a host with none says so rather than failing to start streams that
/// do not exist.
pub const HAS_OS_TRACE: bool = cfg!(any(target_os = "macos", windows));

/// Things the OS wants done once, first thing in `main`: on Windows, becoming DPI aware so that
/// sizes and positions are in physical pixels. Nothing on other hosts.
pub fn init() {
    imp::init();
}

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Which master clock a session's `*_mach_ns` values are on.
///
/// The field names keep their historical `mach_` spelling so that sessions stay readable by every
/// build of the tool; what they hold is "nanoseconds on the host's monotonic clock", and this says
/// which clock that was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ClockKind {
    /// `mach_absolute_time`: pauses while the machine sleeps.
    Mach,
    /// `QueryPerformanceCounter`.
    Qpc,
    /// `CLOCK_MONOTONIC`: pauses while the machine is suspended.
    Monotonic,
    /// A host with no capture, which still needs some monotonic clock to run on.
    Std,
}

/// What a session was recorded on.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::platform::ClockKind;
/// use komorebi_recorder::platform::Host;
///
/// // a session from before hosts were recorded can only have been recorded on a Mac
/// let host = Host::legacy_macos(Some("26.0".to_string()));
/// assert!(host.is_macos() && !host.is_windows());
/// assert_eq!(host.clock, ClockKind::Mach);
/// assert_eq!(host.describe(), "macOS 26.0");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Host {
    /// `macos`, `windows`, ...
    pub os: String,
    /// The CPU architecture, as Rust names it (`aarch64`, `x86_64`).
    pub arch: String,
    /// The OS version, if the host could say (`26.0` on macOS, `10.0.26100` on Windows).
    pub os_version: Option<String>,
    /// Which clock the session's `*_mach_ns` values are on.
    pub clock: ClockKind,
}

impl Host {
    /// The machine this process is running on.
    #[must_use]
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            os_version: shell::os_version(),
            clock: clock::KIND,
        }
    }

    /// What a session from before hosts were recorded was recorded on: it can only have been a Mac.
    #[must_use]
    pub fn legacy_macos(os_version: Option<String>) -> Self {
        Self {
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
            os_version,
            clock: ClockKind::Mach,
        }
    }

    #[must_use]
    /// Returns `true` if and only if this is macOS.
    pub fn is_macos(&self) -> bool {
        self.os == "macos"
    }

    #[must_use]
    /// Returns `true` if and only if this is Windows.
    pub fn is_windows(&self) -> bool {
        self.os == "windows"
    }

    #[must_use]
    /// Returns `true` if and only if this is Linux.
    pub fn is_linux(&self) -> bool {
        self.os == "linux"
    }

    /// `macOS 26.0`, `windows 10.0.26100`: for prose.
    #[must_use]
    pub fn describe(&self) -> String {
        let os = match self.os.as_str() {
            "macos" => "macOS",
            "windows" => "Windows",
            "linux" => "Linux",
            other => other,
        };
        match &self.os_version {
            Some(version) => format!("{os} {version}"),
            None => os.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_reads_back_what_it_wrote() {
        let host = Host::current();
        let text = serde_json::to_string(&host).expect("serialize");
        let back: Host = serde_json::from_str(&text).expect("parse");
        assert_eq!(back, host);
    }

    #[test]
    fn the_clock_kind_is_spelled_in_lowercase() {
        assert_eq!(
            serde_json::to_string(&ClockKind::Qpc).expect("serialize"),
            r#""qpc""#
        );
        assert_eq!(
            serde_json::to_string(&ClockKind::Mach).expect("serialize"),
            r#""mach""#
        );
    }

    #[test]
    fn a_session_from_before_hosts_were_recorded_was_a_mac() {
        let host = Host::legacy_macos(Some("26.0".into()));
        assert!(host.is_macos());
        assert_eq!(host.describe(), "macOS 26.0");
        assert_eq!(host.clock, ClockKind::Mach);
    }
}
