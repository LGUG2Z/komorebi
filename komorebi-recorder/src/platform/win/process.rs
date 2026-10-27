//! Starting and stopping child processes the way Windows wants it.
//!
//! The recorder window launches the viewer, and ffmpeg is run for some video work. [`isolate`]
//! starts a child so that a terminal's Ctrl-C reaches only the recorder and no console window
//! flashes, [`terminate`] stops it and [`was_terminated`] tells a child that was stopped from one
//! that failed. The same three functions exist on macOS, where a child can be asked to stop.

use std::os::windows::process::CommandExt;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Starts the child in its own process group, so a terminal Ctrl-C reaches only the recorder, and
/// without a console window of its own, so that the recorder window launching `ffmpeg` does not
/// flash one.
///
/// This must be called before the child is spawned (it sets the creation flags
/// `CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`, replacing any set before). It returns `command`
/// for chaining.
pub fn isolate(command: &mut Command) -> &mut Command {
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
}

/// Stops a child, without giving it a chance to flush.
///
/// Windows has no polite signal for a process without a window, so this is `TerminateProcess`:
/// whatever the child had buffered is lost. A failure to stop it is not reported. On macOS the
/// child is sent `SIGTERM` and can flush.
pub fn terminate(child: &mut Child) {
    let _ = child.kill();
}

/// Returns `true` if and only if the child ended because [`terminate`] asked it to.
///
/// Windows reports no signal, so this cannot tell, and always returns `false`. On macOS it is
/// `true` for a child that was stopped by the signal [`terminate`] sends.
#[must_use]
pub fn was_terminated(_status: &ExitStatus) -> bool {
    false
}
