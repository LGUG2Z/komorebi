//! Starting and stopping child processes the way macOS wants it.
//!
//! The recorder runs `log stream` children (`crate::sources::oslog`) and, from the recorder window,
//! the viewer. [`isolate`] starts one so that a terminal's Ctrl-C reaches only the recorder,
//! [`terminate`] asks it to stop and [`was_terminated`] tells a child that was asked to stop from
//! one that failed. The same three functions exist on Windows, where the means differ.

use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;

/// Puts the child in its own process group, so a terminal Ctrl-C reaches only the recorder, which
/// can then drain and stop these children in order instead of losing their tail.
///
/// This must be called before the child is spawned. It returns `command` for chaining. On Windows
/// the child also gets no console window of its own.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::platform::process::{isolate, terminate, was_terminated};
/// use std::process::Command;
///
/// let mut command = Command::new("sleep");
/// command.arg("30");
/// let mut child = isolate(&mut command).spawn()?;
///
/// // asked to stop with SIGTERM, which `was_terminated` recognizes
/// terminate(&mut child);
/// let status = child.wait()?;
/// assert!(!status.success());
/// assert!(was_terminated(&status));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn isolate(command: &mut Command) -> &mut Command {
    std::os::unix::process::CommandExt::process_group(command, 0)
}

/// Asks a child to stop, as politely as the OS allows (`SIGTERM`), so it flushes what it buffered.
///
/// This only sends the signal: it does not wait for the child, so the caller must `wait` for it,
/// and does not report a failure to signal it. On Windows there is no polite signal, and the child
/// is killed.
pub fn terminate(child: &mut Child) {
    // SAFETY: signalling a child process we spawned and still own
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
}

/// Returns `true` if and only if the child ended because [`terminate`] asked it to (it was
/// killed by `SIGTERM`), which is not a failure.
///
/// On Windows this is always `false`: Windows reports no signal, so it cannot tell.
#[must_use]
pub fn was_terminated(status: &ExitStatus) -> bool {
    std::os::unix::process::ExitStatusExt::signal(status) == Some(libc::SIGTERM)
}
