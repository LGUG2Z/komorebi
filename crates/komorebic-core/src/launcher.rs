//! `komorebic start` and `komorebic stop`: the part of them that is the same on every platform.
//!
//! Starting the daemon is the same dance everywhere: look whether it is running, and if it is
//! not, start it, wait a few seconds, look again, and give up after three attempts by running it
//! in front of the user so that what it says on its standard error can be read. What differs is
//! how the daemon is started (a `Command`, a PowerShell script), how its process is found, and
//! what is done once it is up (companions such as whkd and the bar, the licence text), which each
//! platform writes by hand around [`start`].

use color_eyre::eyre;
use komorebi_protocol::message::SocketMessage;
use std::io::Write;
use std::time::Duration;

/// How many times the daemon is started before `start` gives up.
pub const ATTEMPTS: usize = 3;

/// How long to wait after each start before looking whether the daemon is running.
pub const WAIT: Duration = Duration::from_secs(3);

/// What `start` needs of a platform.
pub trait Launcher {
    /// What the daemon is called in what is printed (`komorebi`, `komorebi.exe`).
    fn daemon_name(&self) -> &str;

    /// Start the daemon in the background.
    fn spawn(&mut self) -> eyre::Result<()>;

    /// Look at the processes and say whether the daemon is among them.
    fn is_running(&mut self) -> eyre::Result<bool>;

    /// Run the daemon in front of the user and print what it says on its standard error.
    fn diagnose(&mut self) -> eyre::Result<()>;
}

/// Start the daemon unless it is running, waiting [`WAIT`] after each of at most [`ATTEMPTS`]
/// starts. Returns whether it is running; if it is not, it has been run in front of the user
/// ([`Launcher::diagnose`]) and the caller has nothing more to do.
pub fn start(launcher: &mut impl Launcher) -> eyre::Result<bool> {
    start_waiting(launcher, WAIT)
}

/// [`start`] with another waiting time.
pub fn start_waiting(launcher: &mut impl Launcher, wait: Duration) -> eyre::Result<bool> {
    let name = launcher.daemon_name().to_string();
    let mut attempts = 0;
    let mut running = launcher.is_running()?;

    while !running && attempts < ATTEMPTS {
        launcher.spawn()?;

        print!("Waiting for {name} to start... ");
        std::io::stdout().flush()?;
        std::thread::sleep(wait);

        if launcher.is_running()? {
            println!("Started!");
            running = true;
        } else {
            println!("{name} did not start... Trying again");
            attempts += 1;
        }
    }

    if !running {
        println!("\nRunning {name} directly for detailed error output\n");
        launcher.diagnose()?;
    }

    Ok(running)
}

/// The message that stops the daemon, with or without restoring the windows it hid.
pub const fn stop_message(ignore_restore: bool) -> SocketMessage {
    if ignore_restore {
        SocketMessage::StopIgnoreRestore
    } else {
        SocketMessage::Stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A daemon that is up after it has been started `starts_needed` times.
    struct Fake {
        starts_needed: usize,
        started: usize,
        diagnosed: bool,
    }

    impl Launcher for Fake {
        fn daemon_name(&self) -> &str {
            "komorebi"
        }

        fn spawn(&mut self) -> eyre::Result<()> {
            self.started += 1;
            Ok(())
        }

        fn is_running(&mut self) -> eyre::Result<bool> {
            Ok(self.started >= self.starts_needed)
        }

        fn diagnose(&mut self) -> eyre::Result<()> {
            self.diagnosed = true;
            Ok(())
        }
    }

    fn fake(starts_needed: usize) -> Fake {
        Fake {
            starts_needed,
            started: 0,
            diagnosed: false,
        }
    }

    #[test]
    fn a_daemon_that_runs_is_not_started() {
        let mut launcher = fake(0);
        assert!(start_waiting(&mut launcher, Duration::ZERO).unwrap());
        assert_eq!(launcher.started, 0);
    }

    #[test]
    fn a_daemon_that_needs_a_second_try_is_started_twice() {
        let mut launcher = fake(2);
        assert!(start_waiting(&mut launcher, Duration::ZERO).unwrap());
        assert_eq!(launcher.started, 2);
        assert!(!launcher.diagnosed);
    }

    #[test]
    fn a_daemon_that_never_starts_is_tried_three_times_and_diagnosed() {
        let mut launcher = fake(usize::MAX);
        assert!(!start_waiting(&mut launcher, Duration::ZERO).unwrap());
        assert_eq!(launcher.started, ATTEMPTS);
        assert!(launcher.diagnosed);
    }

    #[test]
    fn stop_says_whether_to_restore() {
        assert!(matches!(stop_message(false), SocketMessage::Stop));
        assert!(matches!(
            stop_message(true),
            SocketMessage::StopIgnoreRestore
        ));
    }
}
