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
