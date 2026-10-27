//! Any other OS: enough to build and to investigate a session directory that was recorded
//! elsewhere (the commands that only read files), but not to record one.

/// Nothing to do first.
pub fn init() {}

/// Which process owns a window: nothing is known on this host.
pub mod apps {
    use crate::sources::managed_apps::WindowOwner;
    use std::collections::HashMap;

    /// Returns no windows: this host has no window server to ask.
    #[must_use]
    pub fn window_owners() -> HashMap<u32, WindowOwner> {
        HashMap::new()
    }
}

/// Screen capture, which this host cannot do.
pub mod capture {
    use crate::capture::CaptureBackend;
    use crate::capture::CaptureConfig;
    use crate::capture::DisplayDescriptor;
    use crate::capture::RunningCapture;
    use color_eyre::eyre;
    use std::path::Path;

    fn unsupported<T>() -> eyre::Result<T> {
        eyre::bail!(
            "recording the screen is not supported on {}; investigating a session recorded \
             elsewhere is",
            std::env::consts::OS
        )
    }

    /// Returns `false`: there is no screen to capture.
    pub fn has_permission() -> bool {
        false
    }

    /// Returns `false`: this host cannot capture the screen.
    #[must_use]
    pub fn supported() -> bool {
        false
    }

    /// Fails: this host cannot capture the screen.
    ///
    /// # Errors
    ///
    /// Always returns an error that says recording is not supported here.
    pub fn ensure_permission() -> eyre::Result<()> {
        unsupported()
    }

    /// Fails: this host has no displays it can capture.
    ///
    /// # Errors
    ///
    /// Always returns an error that says recording is not supported here.
    pub fn active_displays() -> eyre::Result<Vec<DisplayDescriptor>> {
        unsupported()
    }

    /// Returns `0`: there is no main display.
    #[must_use]
    pub fn main_display_id() -> u32 {
        0
    }

    /// Fails: this host has no main display.
    ///
    /// # Errors
    ///
    /// Always returns an error that says recording is not supported here.
    pub fn main_display() -> eyre::Result<DisplayDescriptor> {
        unsupported()
    }

    /// Returns `None`: this host has nothing to read a video's duration with.
    #[must_use]
    pub fn media_duration_s(_path: &Path) -> Option<f64> {
        None
    }

    /// A capture backend that cannot capture.
    pub struct Backend;

    impl CaptureBackend for Backend {
        fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
            active_displays()
        }

        fn start(
            &self,
            _display: &DisplayDescriptor,
            _segment: u32,
            _config: &CaptureConfig,
            _dir: &Path,
        ) -> eyre::Result<Box<dyn RunningCapture>> {
            unsupported()
        }
    }
}

/// The master clock, which here is Rust's monotonic [`Instant`].
pub mod clock {
    use crate::clock::Timebase;
    use crate::platform::ClockKind;
    use std::sync::OnceLock;
    use std::time::Instant;

    /// The clock a session recorded here is on: Rust's [`Instant`].
    pub const KIND: ClockKind = ClockKind::Std;

    /// Returns the identity timebase: [`now_ns`] is already in nanoseconds.
    #[must_use]
    pub fn timebase() -> Timebase {
        Timebase { numer: 1, denom: 1 }
    }

    /// Returns no other clocks.
    #[must_use]
    pub fn reference_clocks() -> Vec<crate::clock::ReferenceClock> {
        Vec::new()
    }

    /// Nanoseconds since the first reading in this process, offset so that none is ever zero
    /// (zero means "not yet" in the capture counters).
    #[must_use]
    pub fn now_ns() -> u64 {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_nanos() as u64 + 1_000_000_000
    }
}

/// The OS trace, which this host does not have.
pub mod trace {
    use crate::sources::wintrace::TraceStream;
    use std::sync::Arc;

    /// Never constructed on this host.
    pub struct Taps;

    /// Returns no taps and the reason: this host has no OS trace.
    #[must_use]
    pub fn start(
        _system: Arc<TraceStream>,
        _event_log: Arc<TraceStream>,
    ) -> (Option<Taps>, Vec<String>) {
        (
            None,
            vec!["winevents: this host has no OS trace".to_string()],
        )
    }
}

/// Starting and stopping child processes, with nothing host-specific to do.
pub mod process {
    use std::process::Child;
    use std::process::Command;
    use std::process::ExitStatus;

    /// Returns `command` as it is: there is nothing to isolate it with.
    pub fn isolate(command: &mut Command) -> &mut Command {
        command
    }

    /// Kills `child`.
    pub fn terminate(child: &mut Child) {
        let _ = child.kill();
    }

    /// Returns `false`: this host reports no signal.
    #[must_use]
    pub fn was_terminated(_status: &ExitStatus) -> bool {
        false
    }
}

/// Showing a folder, asking for one, and naming the OS, with what this host can offer.
pub mod shell {
    use std::path::Path;
    use std::path::PathBuf;

    /// What this host calls its file manager, for button labels.
    pub const FILE_MANAGER: &str = "file manager";

    /// Returns `None`: this host cannot say which version it is.
    #[must_use]
    pub fn os_version() -> Option<String> {
        None
    }

    /// Asks `xdg-open` to show `path`.
    pub fn reveal(path: &Path) {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }

    /// Returns `None`: this host has no dialog to ask with.
    #[must_use]
    pub fn pick_folder(_prompt: &str) -> Option<PathBuf> {
        None
    }
}
