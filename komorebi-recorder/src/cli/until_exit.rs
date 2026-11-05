//! `record --until-exit -- <command>`: run a command while recording, and stop when it exits.
//!
//! The recording is the point and the command is what is being recorded: a script that reproduces a
//! bug, run so that the screen, komorebi's log and its events are captured exactly while it runs.
//! [`supervise`] starts the command once the recording is under way (so that the session directory
//! exists), with [`SESSION_ENV`] set to that directory, and watches it. The command inherits the
//! recorder's standard streams and working directory, so its output is visible and relative paths
//! mean what the user meant.
//!
//! * When the command exits, the recording is asked to stop.
//! * When the recording ends first (Ctrl-C, `--duration`), the command is terminated.
//!
//! The recorder's exit status is about the recording, not the command: it is 0 when the session was
//! finalised, whatever the command's status was. The command's status is in the summary and in
//! `record --json` ([`ChildReport`]).

use super::outputs::ChildEnd;
use super::outputs::ChildReport;
use crate::platform::process;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The environment variable the command is given: the session directory being recorded to, as an
/// absolute path. `mark` and `annotate` read it when they are not given a session.
pub const SESSION_ENV: &str = "KOMOREBI_RECORDER_SESSION";

/// How often the command is looked at.
const POLL: Duration = Duration::from_millis(50);

/// Runs `command` (a program and its arguments) with [`SESSION_ENV`] set to `session_dir`, until it
/// exits or `recording_over` says the recording has ended, and says what became of it.
///
/// `on_exit` is called when the command exits (or cannot be started), and is how the caller stops
/// the recording. `recording_over` is asked every 50 ms while the command runs; when it is true the
/// command is terminated (see [`process::terminate`]) and waited for. This blocks until the command
/// has gone.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::cli::outputs::ChildEnd;
/// use komorebi_recorder::cli::until_exit::supervise;
/// use std::sync::atomic::{AtomicBool, Ordering};
///
/// let stopped = AtomicBool::new(false);
/// let command: Vec<String> = if cfg!(windows) {
///     ["cmd", "/c", "exit 3"].map(String::from).to_vec()
/// } else {
///     ["sh", "-c", "exit 3"].map(String::from).to_vec()
/// };
///
/// let report = supervise(
///     &command,
///     std::path::Path::new("some-session"),
///     || false,
///     || stopped.store(true, Ordering::SeqCst),
/// );
///
/// // the command's own status is reported, and the recording was asked to stop
/// assert_eq!(report.end, ChildEnd::Exited);
/// assert_eq!(report.exit_code, Some(3));
/// assert_eq!(report.success, Some(false));
/// assert!(stopped.load(Ordering::SeqCst));
/// ```
pub fn supervise(
    command: &[String],
    session_dir: &Path,
    recording_over: impl Fn() -> bool,
    on_exit: impl Fn(),
) -> ChildReport {
    let report = |end, exit_code: Option<i32>, error: Option<String>| ChildReport {
        command: command.to_vec(),
        end,
        exit_code,
        success: (end == ChildEnd::Exited).then(|| exit_code == Some(0)),
        error,
    };

    let Some((program, arguments)) = command.split_first() else {
        on_exit();
        return report(
            ChildEnd::FailedToStart,
            None,
            Some("there is no command to run".to_string()),
        );
    };

    let absolute = std::path::absolute(session_dir).unwrap_or_else(|_| session_dir.to_path_buf());
    let mut child = match process::isolate(
        Command::new(program)
            .args(arguments)
            .env(SESSION_ENV, absolute),
    )
    .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            on_exit();
            return report(
                ChildEnd::FailedToStart,
                None,
                Some(format!("could not start {program}: {error}")),
            );
        }
    };

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                on_exit();
                return report(ChildEnd::Exited, status.code(), None);
            }
            Ok(None) => {}
            Err(error) => {
                process::terminate(&mut child);
                let _ = child.wait();
                on_exit();
                return report(
                    ChildEnd::FailedToStart,
                    None,
                    Some(format!("could not wait for {program}: {error}")),
                );
            }
        }

        if recording_over() {
            process::terminate(&mut child);
            let _ = child.wait();
            return report(ChildEnd::Stopped, None, None);
        }

        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    fn shell(script: &str) -> Vec<String> {
        if cfg!(windows) {
            vec!["cmd".into(), "/c".into(), script.into()]
        } else {
            vec!["sh".into(), "-c".into(), script.into()]
        }
    }

    #[test]
    fn a_command_that_exits_stops_the_recording_and_keeps_its_status() {
        let stop = AtomicBool::new(false);
        let report = supervise(
            &shell("exit 0"),
            Path::new("."),
            || false,
            || stop.store(true, Ordering::SeqCst),
        );

        assert_eq!(report.end, ChildEnd::Exited);
        assert_eq!(report.exit_code, Some(0));
        assert_eq!(report.success, Some(true));
        assert!(stop.load(Ordering::SeqCst));
    }

    #[test]
    fn the_command_is_told_which_session_it_is_recorded_in() {
        let check = if cfg!(windows) {
            format!("if defined {SESSION_ENV} (exit 0) else (exit 7)")
        } else {
            format!("[ -n \"${SESSION_ENV}\" ] && exit 0 || exit 7")
        };

        let report = supervise(&shell(&check), Path::new("a-session"), || false, || {});
        assert_eq!(report.exit_code, Some(0), "{report:?}");
    }

    #[test]
    fn a_recording_that_ends_first_terminates_the_command() {
        let sleeper = if cfg!(windows) {
            shell("ping -n 30 127.0.0.1 >nul")
        } else {
            shell("sleep 30")
        };
        let started = std::time::Instant::now();
        let report = supervise(&sleeper, Path::new("."), || true, || {});

        assert_eq!(report.end, ChildEnd::Stopped);
        assert_eq!(report.exit_code, None);
        assert_eq!(report.success, None);
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn a_command_that_cannot_start_is_reported_and_stops_the_recording() {
        let stop = AtomicBool::new(false);
        let report = supervise(
            &["komorebi-recorder-no-such-program".to_string()],
            Path::new("."),
            || false,
            || stop.store(true, Ordering::SeqCst),
        );

        assert_eq!(report.end, ChildEnd::FailedToStart);
        assert!(
            report
                .error
                .is_some_and(|error| error.contains("could not start"))
        );
        assert!(stop.load(Ordering::SeqCst));
    }
}
