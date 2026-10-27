//! Telling a recorder that is running from one that died.
//!
//! A session's manifest says `recording` both while a recorder is alive and after one was killed,
//! and the right thing to do differs: wait, or `reindex`. The recorder therefore holds an exclusive
//! lock on `recorder.lock` for as long as it runs. The OS drops the lock the instant the process
//! dies, however it dies, so a lock that is still held means a live recorder and one that is free
//! means there is none.
//!
//! The pid lives in a file of its own, `recorder.pid`. Windows file locks are mandatory, so a
//! reader could not read a pid out of the very file that is locked; keeping the lock file empty
//! works the same on every OS.
//!
//! # The two files
//!
//! | File | Holds |
//! |---|---|
//! | [`LOCK_FILE`], `recorder.lock` | nothing: it is empty, and what matters is whether a lock is held on it |
//! | [`PID_FILE`], `recorder.pid` | the recorder's process id as decimal text and a newline, replaced by each recorder that takes the lock |
//!
//! The lock is [`File::try_lock`](std::fs::File::try_lock), which is `flock` on unix and
//! `LockFileEx` on Windows. It is held by an open file, not recorded as a pid, so it cannot go
//! stale.
//!
//! # Who uses this
//!
//! A recorder takes the lock with [`RecorderLock::acquire`] when it begins to record. Anything that
//! meets a session whose manifest says `recording` asks [`recorder`] whether that is a recorder at
//! work (wait for it) or one that died (`reindex` the session). This module is the same on every
//! host.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::lock::{recorder, Recorder, RecorderLock};
//!
//! let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-lock-{}", std::process::id()));
//! std::fs::create_dir_all(&dir)?;
//!
//! // no lock file: a session that predates it, or no session at all
//! assert_eq!(recorder(&dir), Recorder::Unknown);
//!
//! // a second recorder cannot take a session that one is recording
//! let lock = RecorderLock::acquire(&dir)?;
//! assert_eq!(recorder(&dir), Recorder::Running(Some(std::process::id())));
//! assert!(RecorderLock::acquire(&dir).is_err());
//!
//! // the lock is released when the recorder is dropped, or dies
//! drop(lock);
//! assert_eq!(recorder(&dir), Recorder::NotRunning);
//! # std::fs::remove_dir_all(&dir)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::fs::File;
use std::fs::OpenOptions;
use std::fs::TryLockError;
use std::path::Path;

/// The file in a session directory that a recording holds a lock on: `recorder.lock`. It is empty.
pub const LOCK_FILE: &str = "recorder.lock";
/// The file in a session directory that has the recorder's process id: `recorder.pid`.
pub const PID_FILE: &str = "recorder.pid";

/// The lock a running recorder holds on its session; released when dropped or when the process
/// ends.
///
/// There is no way to hold it without a recorder: the value is made by [`RecorderLock::acquire`],
/// and keeps the lock file open for as long as it lives. Its only field is private, so no other
/// code can make one around a file of its choosing:
///
/// ```rust,compile_fail,E0451
/// use komorebi_recorder::lock::RecorderLock;
///
/// let file = std::fs::File::create("recorder.lock").expect("a file");
/// let _lock = RecorderLock { _file: file };
/// ```
#[derive(Debug)]
pub struct RecorderLock {
    _file: File,
}

impl RecorderLock {
    /// Takes the lock for the session in `dir`, and writes this process's id next to it.
    ///
    /// The lock file is created if it does not exist. The pid file is only written once the lock
    /// is held, so it is never replaced by a process that did not get the lock.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if
    ///
    /// * the lock file cannot be created or opened: `dir` does not exist, or cannot be written to;
    /// * another recorder holds the lock: the kind is [`WouldBlock`](std::io::ErrorKind::WouldBlock)
    ///   and the message is `another recorder is already recording this session`;
    /// * the lock cannot be taken for another reason, such as a file system that does not support
    ///   locks;
    /// * the pid file cannot be written. The lock is released again in that case.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::lock::RecorderLock;
    ///
    /// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-acquire-{}", std::process::id()));
    /// std::fs::create_dir_all(&dir)?;
    ///
    /// let _first = RecorderLock::acquire(&dir)?;
    /// let error = RecorderLock::acquire(&dir).unwrap_err();
    /// assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    /// assert_eq!(error.to_string(), "another recorder is already recording this session");
    /// # std::fs::remove_dir_all(&dir)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn acquire(dir: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK_FILE))?;

        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "another recorder is already recording this session",
                ));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }

        // only now that the lock is ours is it safe to replace what the previous owner wrote
        std::fs::write(dir.join(PID_FILE), format!("{}\n", std::process::id()))?;
        Ok(Self { _file: file })
    }
}

/// What can be told about the recorder of a session from its lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorder {
    /// A recorder holds the lock; the pid it wrote, if readable.
    Running(Option<u32>),
    /// The lock file exists and nobody holds it.
    NotRunning,
    /// There is no lock file: the session predates it.
    Unknown,
}

/// Returns whether a recorder is at work on the session in `dir`, by asking whether its lock is
/// held.
///
/// The lock is tried with a shared lock, and let go of at once. [`Recorder::Running`] therefore
/// means that some open file holds the lock, which includes one held by the calling process.
/// [`Recorder::NotRunning`] means the lock file is there and free, and is also what a lock that
/// cannot be queried at all (a file system without locks, say) reads as: that is no evidence of a
/// live recorder, and calling a dead session live would make `reindex` refuse it.
/// [`Recorder::Unknown`] is for a lock file that is missing, or cannot be opened.
///
/// A recorder that was killed leaves its lock file and pid file behind, but not the lock, so the
/// session reads as [`NotRunning`](Recorder::NotRunning).
///
/// This never fails and never blocks. It is a snapshot: the answer can be out of date by the time
/// it is read.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::lock::{recorder, Recorder};
///
/// let dir = std::env::temp_dir().join(format!("komorebi-recorder-doc-recorder-{}", std::process::id()));
/// std::fs::create_dir_all(&dir)?;
/// assert_eq!(recorder(&dir), Recorder::Unknown);
/// # std::fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// What each answer is for a live recorder, a dead one and a session with no lock file is covered
/// by the tests `the_lock_is_held_while_the_recorder_lives_and_free_after`,
/// `a_session_without_a_lock_file_is_unknown` and
/// `a_pid_written_into_the_lock_file_by_an_older_build_is_still_read`.
#[must_use]
pub fn recorder(dir: &Path) -> Recorder {
    let path = dir.join(LOCK_FILE);
    let Ok(file) = File::open(&path) else {
        return Recorder::Unknown;
    };

    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            Recorder::NotRunning
        }
        // anything but "someone holds it" (a filesystem without locks, say) is not evidence of a
        // live recorder, and calling a dead session live would make `reindex` refuse it
        Err(TryLockError::Error(_)) => Recorder::NotRunning,
        Err(TryLockError::WouldBlock) => Recorder::Running(pid(dir)),
    }
}

/// The pid a recorder wrote: in `recorder.pid`, or else in the lock file itself, where a recorder
/// that wrote the pid there left it (readable only on an OS whose locks are advisory).
fn pid(dir: &Path) -> Option<u32> {
    [PID_FILE, LOCK_FILE].iter().find_map(|name| {
        std::fs::read_to_string(dir.join(name))
            .ok()
            .and_then(|text| text.trim().parse().ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-lock-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn a_session_without_a_lock_file_is_unknown() {
        assert_eq!(recorder(&temp("none")), Recorder::Unknown);
    }

    #[test]
    fn the_lock_is_held_while_the_recorder_lives_and_free_after() {
        let dir = temp("held");
        let lock = RecorderLock::acquire(&dir).expect("acquire");

        assert_eq!(recorder(&dir), Recorder::Running(Some(std::process::id())));

        drop(lock);
        // a process spawned at that moment by another test briefly holds a copy of the descriptor,
        // and a lock lasts until every copy of the descriptor is closed
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while recorder(&dir) != Recorder::NotRunning && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(recorder(&dir), Recorder::NotRunning);
    }

    #[test]
    fn the_pid_is_kept_out_of_the_locked_file() {
        let dir = temp("pidfile");
        let _lock = RecorderLock::acquire(&dir).expect("acquire");

        // Windows would refuse to read a file that is locked, so the pid cannot live in it
        assert_eq!(
            std::fs::read_to_string(dir.join(LOCK_FILE)).expect("lock file"),
            ""
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(PID_FILE))
                .expect("pid file")
                .trim(),
            std::process::id().to_string()
        );
    }

    #[test]
    fn a_pid_written_into_the_lock_file_by_an_older_build_is_still_read() {
        let dir = temp("legacy");
        let lock = File::create(dir.join(LOCK_FILE)).expect("lock file");
        std::fs::write(dir.join(LOCK_FILE), "4242\n").expect("legacy pid");
        lock.try_lock().expect("held");

        assert_eq!(recorder(&dir), Recorder::Running(Some(4242)));
    }

    #[test]
    fn two_recorders_cannot_share_a_session() {
        let dir = temp("twice");
        let _first = RecorderLock::acquire(&dir).expect("first");
        assert!(RecorderLock::acquire(&dir).is_err());
    }
}
