//! What this program's windows do about being in the video and being tiled, which differs by OS.
//!
//! The recorder window ([`crate::app`]) and the viewer must not be in the video they are
//! recording, and komorebi must not tile them. The two hosts get there differently:
//!
//! | | macOS | Windows |
//! |---|---|---|
//! | left out of the video | the recorder excludes its own processes from the capture by name ([`crate::capture`]) | each window asks to be left out of captures, because Windows captures a whole monitor |
//! | left alone by komorebi | the windows are created with no title, which komorebi for Mac does not manage unless the program is one of its titleless applications | the windows have no title here too; whether komorebi for Windows leaves an untitled window alone is what [`PROBE_VARIABLE`] finds out on a real machine |
//!
//! A window's update loop calls [`Chrome::tick`], which does the part that needs doing again and
//! again on Windows: windows are created after the last call (the viewer opened from the recorder,
//! a dialog), so the request to be left out of captures is repeated. On macOS it does nothing.
//!
//! [`inspect`] is a separate matter: it reads what Windows says about any window handle, for the
//! per-event probe (`crate::sources::winprobe`), and is `None` on every other host.

use crate::komorebi_link;
use crate::sources::winprobe::WindowFacts;
use std::time::Duration;
use std::time::Instant;

/// How often the windows are asked to be excluded again: a window created after the last time (the
/// viewer opened from the recorder, a dialog) is caught by the next.
const TICK: Duration = Duration::from_secs(1);

/// How long after the first tick komorebi is asked about this program's windows, so that they
/// have been shown and komorebi has had its chance to act on them.
const PROBE_AFTER: Duration = Duration::from_secs(3);

/// The environment variable that makes a window ask komorebi about itself.
///
/// With `KOMOREBI_RECORDER_PROBE_WINDOW` set (to any value, `1` by convention), a window asks
/// komorebi whether it would manage it, three seconds after [`Chrome`] first ticks, and writes the
/// answer to `komorebi-recorder-window-probe.txt` in the temp directory: how to find out on a
/// real machine whether the window is left alone, and what komorebi says if not. On a host that
/// has no such windows the file says so.
pub const PROBE_VARIABLE: &str = "KOMOREBI_RECORDER_PROBE_WINDOW";

/// Returns what Windows says about the window with handle `hwnd` right now (see
/// `crate::sources::winprobe`), or `None` on a host that has no such windows to ask about.
///
/// On Windows the answer is always `Some`, even for a handle that names no window: that is a
/// `WindowFacts` whose `exists` is `false`. The rect it carries is the visible frame, which is how
/// komorebi reads one, so the two can be compared. `at_ns` and `komorebi_says` are not filled in;
/// the prober that calls this sets them.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::platform::window::inspect;
///
/// // only Windows has windows to ask about
/// assert_eq!(inspect(0x20000).is_some(), cfg!(windows));
/// ```
#[must_use]
pub fn inspect(hwnd: u32) -> Option<WindowFacts> {
    #[cfg(windows)]
    {
        super::win::window::inspect(hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        None
    }
}

/// Per-window housekeeping for a window's update loop. Call [`tick`](Chrome::tick) on every frame:
/// it is cheap, and does something about once a second.
///
/// A `Chrome` has no automated test: what it does is act on the windows of the running process.
/// On Windows it is checked by running the recorder (see `docs/dev/windows-checkpoints.md`).
pub struct Chrome {
    last: Instant,
    began: Instant,
    probed: bool,
}

impl Default for Chrome {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            // due at once
            last: now.checked_sub(TICK).unwrap_or(now),
            began: now,
            probed: false,
        }
    }
}

impl Chrome {
    /// Does what is due, if anything: at most once a second, and the first call at once.
    ///
    /// On Windows each of the process's visible top-level windows is asked to be left out of
    /// screen captures, which a window created since the last tick needs. A Windows too old for
    /// it (before Windows 10 version 2004) refuses, and that is not reported here; `doctor` says
    /// so. If [`PROBE_VARIABLE`] is set, the first tick at least three seconds after the `Chrome`
    /// was created also asks komorebi about the process's windows, once, and writes the answer to
    /// `komorebi-recorder-window-probe.txt` in the temp directory. On other hosts only the probe
    /// does anything, and it writes that this program has no windows komorebi could know.
    pub fn tick(&mut self) {
        if self.last.elapsed() < TICK {
            return;
        }
        self.last = Instant::now();

        // an older Windows cannot do this, which is not worth interrupting anyone for; `doctor`
        // says so
        #[cfg(windows)]
        let _ = super::win::window::exclude_from_capture();

        if !self.probed && self.began.elapsed() >= PROBE_AFTER {
            self.probed = true;
            if std::env::var_os(PROBE_VARIABLE).is_some() {
                probe();
            }
        }
    }
}

/// Ask komorebi about this program's windows and write what it says down.
fn probe() {
    #[cfg(windows)]
    let handles = super::win::window::own_window_handles();
    #[cfg(not(windows))]
    let handles: Vec<u32> = Vec::new();

    let mut text = String::new();
    if handles.is_empty() {
        text.push_str("this program has no visible windows of its own that komorebi could know\n");
    }
    for hwnd in handles {
        text.push_str(&format!("window {hwnd}:\n"));
        match komorebi_link::send_query(&komorebi_link::Message::DebugWindow(hwnd)) {
            Ok(reply) if reply.trim().is_empty() => {
                text.push_str("  komorebi gave no answer (busy, paused, or no such window)\n");
            }
            Ok(reply) => {
                text.push_str("  komorebi says:\n");
                text.push_str(&reply);
                text.push('\n');
            }
            Err(error) => text.push_str(&format!("  could not ask komorebi: {error}\n")),
        }
    }

    let _ = std::fs::write(
        std::env::temp_dir().join("komorebi-recorder-window-probe.txt"),
        text,
    );
}
