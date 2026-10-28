//! Naming the recorder's windows, and telling komorebi to leave them alone.
//!
//! The recorder window and the viewer are used on the machine whose behavior is being investigated, so komorebi
//! must not tile them. For a long time that was done by giving them no title at all, which komorebi does not
//! manage. This module lets them have a real title, `komorebi-recorder`, on one condition: _the window is only
//! named when it is safely ignored_.
//!
//! - **komorebi for Windows is running and `komorebic` can be found**: the recorder runs
//!   `komorebic ignore-rule exe <name>` for its own executables before it creates a window, so komorebi never sees a
//!   titled window that it has not been told to ignore. That is komorebi's live runtime rule: nothing is written to
//!   `komorebi.json`, and running it on every start is harmless. The window is titled.
//! - **komorebi is not running**: there is nothing to tile the window, so it is titled, and nothing is said.
//! - **`komorebic` is missing, or the rule could not be registered**: the window stays untitled, as it always was,
//!   and the recorder says so once on standard error.
//! - **Anywhere that is not Windows**: komorebi for Mac manages any window that has a title and has no ignore rule
//!   to rely on, so the windows stay untitled.
//!
//! The decisions are plain functions of an [`Outcome`], and the commands run through a [`Runner`], so all of it is
//! tested with a fake one:
//!
//! ```rust
//! use komorebi_recorder::komorebi_ignore::{title_for, Outcome};
//!
//! assert_eq!(title_for(&Outcome::Registered, true), "komorebi-recorder");
//! assert_eq!(title_for(&Outcome::KomorebicNotFound, true), "");
//! assert_eq!(title_for(&Outcome::Registered, false), "");
//! ```

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

/// The title of the windows when it is safe to give them one.
pub const TITLE: &str = "komorebi-recorder";

/// The executable of the windows that have no console, which the recorder window opens the viewer with.
pub const GUI_EXE: &str = "komorebi-recorder-gui.exe";

/// How long `komorebic ignore-rule` has before it is killed.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// What came of asking komorebi to ignore the recorder's windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `komorebic ignore-rule` succeeded for every executable.
    Registered,
    /// No komorebi is running, so nothing will tile the windows.
    KomorebiNotRunning,
    /// komorebi is running but `komorebic` is neither next to it nor on `PATH`.
    KomorebicNotFound,
    /// `komorebic` was found and did not succeed: the reason.
    Failed(String),
    /// This host has no ignore rule to register (it is not Windows).
    NotApplicable,
}

/// How a started command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ran {
    /// It exited with this code (`-1` if it was ended by a signal).
    Exited(i32),
    /// It did not finish within the time allowed and was killed.
    TimedOut,
}

/// Everything the policy needs from the machine, so that it can be tested without one.
pub trait Runner {
    /// The running komorebi: `None` if there is none, `Some(None)` if it runs and its binary is not known.
    fn komorebi(&mut self) -> Option<Option<PathBuf>>;
    /// Whether `path` is a file.
    fn is_file(&mut self, path: &Path) -> bool;
    /// The directories of `PATH`.
    fn path_dirs(&mut self) -> Vec<PathBuf>;
    /// Runs `program` with `args`, with no console window, killing it after `timeout`.
    ///
    /// # Errors
    ///
    /// Returns why the program could not be started.
    fn run(&mut self, program: &Path, args: &[String], timeout: Duration) -> Result<Ran, String>;
}

/// What a check that runs nothing found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// komorebi runs and this is its `komorebic`.
    Ready(PathBuf),
    /// No komorebi is running.
    KomorebiNotRunning,
    /// komorebi runs and `komorebic` was not found.
    KomorebicNotFound,
    /// This host has no ignore rule to register.
    NotApplicable,
}

/// The executables of the recorder's windows: `current` (this one) and, if it is another, [`GUI_EXE`].
#[must_use]
pub fn exe_names(current: &str) -> Vec<String> {
    let mut names = vec![current.to_string()];
    if !current.eq_ignore_ascii_case(GUI_EXE) {
        names.push(GUI_EXE.to_string());
    }
    names
}

/// The arguments of `komorebic` that ignore the windows of the executable `name`.
#[must_use]
pub fn ignore_args(name: &str) -> Vec<String> {
    ["ignore-rule", "exe", name]
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// Looks without running anything: is komorebi running, and where is its `komorebic`?
///
/// `komorebic.exe` next to the running komorebi comes first, then `PATH`. `windows` is whether the host is
/// Windows; anywhere else there is nothing to do.
pub fn check(runner: &mut dyn Runner, windows: bool) -> Readiness {
    if !windows {
        return Readiness::NotApplicable;
    }
    let Some(exe) = runner.komorebi() else {
        return Readiness::KomorebiNotRunning;
    };
    match candidates(runner, exe.as_deref()).into_iter().next() {
        Some(candidate) => Readiness::Ready(candidate),
        None => Readiness::KomorebicNotFound,
    }
}

/// Every `komorebic.exe` that exists, best first: the one next to the running komorebi (`komorebi`, if its file
/// is known), then those on `PATH`, each once.
///
/// More than one is common on a machine that builds komorebi: a debug build next to a dev komorebi, and a release
/// build from `cargo install` on `PATH`. A debug build can be unusable (see [`describe_exit`]), so
/// [`ensure_with`] tries them in turn.
fn candidates(runner: &mut dyn Runner, komorebi: Option<&Path>) -> Vec<PathBuf> {
    let mut wanted = Vec::new();
    if let Some(dir) = komorebi.and_then(Path::parent) {
        wanted.push(dir.join("komorebic.exe"));
    }
    for dir in runner.path_dirs() {
        wanted.push(dir.join("komorebic.exe"));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in wanted {
        let seen = found.iter().any(|known| {
            known
                .as_os_str()
                .eq_ignore_ascii_case(candidate.as_os_str())
        });
        if !seen && runner.is_file(&candidate) {
            found.push(candidate);
        }
    }
    found
}

/// Says how a command's exit code should be read.
///
/// A code of 0 to 255 is the command's own answer. Anything else on Windows is a status from the system: the
/// command crashed instead of answering, so the code is named, which is the difference between "komorebi refused
/// the rule" and "komorebic is broken". The numbers are `NTSTATUS` values.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::komorebi_ignore::describe_exit;
///
/// assert_eq!(describe_exit(1), "exited with 1");
/// assert!(describe_exit(-1073741571).contains("overflowed its stack"));
/// assert!(describe_exit(-1073741819).contains("access violation"));
/// assert_eq!(describe_exit(-2), "crashed with status 0xfffffffe");
/// ```
#[must_use]
pub fn describe_exit(code: i32) -> String {
    if (0..=255).contains(&code) {
        return format!("exited with {code}");
    }
    let status = code as u32;
    match status {
        0xC000_00FD => format!("crashed with status {status:#x} (it overflowed its stack)"),
        0xC000_0005 => format!("crashed with status {status:#x} (an access violation)"),
        0xC000_0135 => {
            format!("could not start, status {status:#x} (a DLL it needs was not found)")
        }
        _ => format!("crashed with status {status:#x}"),
    }
}

/// Registers an ignore rule for each of the recorder's executables, if komorebi is running and a `komorebic` is
/// found. Pure apart from what `runner` does; `current` is this executable's file name.
///
/// Each `komorebic` that exists is tried in turn ([`candidates`]), and the first that registers every rule
/// wins. One that crashes or refuses does not end the attempt while another is left; if none works the
/// failure names every one tried and how it failed.
pub fn ensure_with(runner: &mut dyn Runner, current: &str, windows: bool) -> Outcome {
    if !windows {
        return Outcome::NotApplicable;
    }
    let Some(exe) = runner.komorebi() else {
        return Outcome::KomorebiNotRunning;
    };
    let candidates = candidates(runner, exe.as_deref());
    if candidates.is_empty() {
        return Outcome::KomorebicNotFound;
    }

    let mut failures = Vec::new();
    for komorebic in candidates {
        match register_all(runner, &komorebic, current) {
            Ok(()) => return Outcome::Registered,
            Err(why) => failures.push(format!("{}: {why}", komorebic.display())),
        }
    }
    Outcome::Failed(failures.join("; then "))
}

/// Runs the ignore rule for each of the recorder's executables with one `komorebic`, stopping at the first that
/// does not work, and says why.
fn register_all(runner: &mut dyn Runner, komorebic: &Path, current: &str) -> Result<(), String> {
    for name in exe_names(current) {
        match runner.run(komorebic, &ignore_args(&name), TIMEOUT) {
            Ok(Ran::Exited(0)) => {}
            Ok(Ran::Exited(code)) => {
                return Err(format!(
                    "`komorebic ignore-rule exe {name}` {}",
                    describe_exit(code)
                ));
            }
            Ok(Ran::TimedOut) => {
                return Err(format!(
                    "`komorebic ignore-rule exe {name}` did not finish in {} seconds",
                    TIMEOUT.as_secs()
                ));
            }
            Err(error) => return Err(format!("could not run it: {error}")),
        }
    }
    Ok(())
}

/// The window title for an outcome: [`TITLE`] only when the window is safely ignored or nothing runs to tile it,
/// and only on Windows (`windows`); otherwise `""`.
#[must_use]
pub fn title_for(outcome: &Outcome, windows: bool) -> &'static str {
    match outcome {
        Outcome::Registered | Outcome::KomorebiNotRunning if windows => TITLE,
        _ => "",
    }
}

/// The note for the user when the title was not applied because the rule could not be registered, or `None` when
/// there is nothing to say (the window is titled, or this host has no rule to register).
#[must_use]
pub fn note(outcome: &Outcome, current: &str) -> Option<String> {
    let why = match outcome {
        Outcome::KomorebicNotFound => "komorebic not found".to_string(),
        Outcome::Failed(error) => error.clone(),
        _ => return None,
    };
    Some(format!(
        "{TITLE}: window left untitled so komorebi will not tile it ({why}); add the rule yourself: komorebic ignore-rule exe {current}"
    ))
}

/// A sentence for `doctor`: whether the windows will be titled and why, from a check that runs nothing.
#[must_use]
pub fn describe(readiness: &Readiness) -> String {
    match readiness {
        Readiness::Ready(path) => format!(
            "titled \"{TITLE}\": the recorder registers an ignore rule with {} first",
            path.display()
        ),
        Readiness::KomorebiNotRunning => {
            format!("titled \"{TITLE}\": komorebi is not running, so nothing tiles them")
        }
        Readiness::KomorebicNotFound => {
            "untitled: komorebi is running and komorebic.exe was not found to ask it to ignore them"
                .to_string()
        }
        Readiness::NotApplicable => {
            "untitled: komorebi for Mac manages any window with a title and has no ignore rule"
                .to_string()
        }
    }
}

/// The real machine: sysinfo for komorebi, the file system, `PATH` and processes.
#[derive(Debug, Default)]
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn komorebi(&mut self) -> Option<Option<PathBuf>> {
        crate::recorder::find_komorebi().map(|process| process.exe)
    }

    fn is_file(&mut self, path: &Path) -> bool {
        path.is_file()
    }

    fn path_dirs(&mut self) -> Vec<PathBuf> {
        std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default()
    }

    fn run(&mut self, program: &Path, args: &[String], timeout: Duration) -> Result<Ran, String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::platform::process::isolate(&mut command);
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(Ran::Exited(status.code().unwrap_or(-1))),
                Ok(None) if started.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(Ran::TimedOut);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => return Err(error.to_string()),
            }
        }
    }
}

/// The file name of this executable, or `komorebi-recorder` if the OS does not say.
fn current_exe_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "komorebi-recorder.exe".to_string()
            } else {
                "komorebi-recorder".to_string()
            }
        })
}

/// Registers the ignore rules for this machine's real komorebi (see the [module documentation](self)). Call it
/// before a window is created.
pub fn ensure_self_ignored() -> Outcome {
    ensure_with(&mut SystemRunner, &current_exe_name(), cfg!(windows))
}

/// What a window needs before it is created: its title, and what to tell the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSetup {
    /// What came of registering the rule.
    pub outcome: Outcome,
    /// The title to build the window with.
    pub title: &'static str,
    /// The line for the user when the title was not applied because the rule could not be registered.
    pub note: Option<String>,
}

/// Runs [`ensure_self_ignored`], prints [`WindowSetup::note`] (if any) on standard error, and returns the title.
pub fn prepare_window() -> WindowSetup {
    let outcome = ensure_self_ignored();
    let note = note(&outcome, &current_exe_name());
    if let Some(note) = &note {
        eprintln!("{note}");
    }
    WindowSetup {
        title: title_for(&outcome, cfg!(windows)),
        outcome,
        note,
    }
}

/// [`describe`] for this machine, running nothing: for `doctor`.
#[must_use]
pub fn describe_here() -> String {
    describe(&check(&mut SystemRunner, cfg!(windows)))
}

/// The rules a user can put in their own komorebi configuration to ignore the recorder's windows, as entries of the
/// application-specific configuration (see [`crate::asc::rule::Rule::preview`]).
#[must_use]
pub fn rule_text() -> String {
    use crate::asc::rule::Identifier;
    use crate::asc::rule::Rule;
    let console = Rule::ignore(
        "komorebi-recorder",
        vec![Identifier::exe("komorebi-recorder.exe")],
    )
    .preview();
    let gui = Rule::ignore("komorebi-recorder-gui", vec![Identifier::exe(GUI_EXE)]).preview();
    format!("{console}\n{gui}")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        komorebi: Option<Option<PathBuf>>,
        files: Vec<PathBuf>,
        path: Vec<PathBuf>,
        result: Result<Ran, String>,
        by_program: Vec<(PathBuf, Result<Ran, String>)>,
        calls: Vec<(PathBuf, Vec<String>, Duration)>,
    }

    impl Fake {
        fn running() -> Self {
            Self {
                komorebi: Some(Some(PathBuf::from("C:/k/komorebi.exe"))),
                files: vec![PathBuf::from("C:/k/komorebic.exe")],
                path: vec![],
                result: Ok(Ran::Exited(0)),
                by_program: vec![],
                calls: vec![],
            }
        }
    }

    impl Runner for Fake {
        fn komorebi(&mut self) -> Option<Option<PathBuf>> {
            self.komorebi.clone()
        }
        fn is_file(&mut self, path: &Path) -> bool {
            self.files.iter().any(|file| file == path)
        }
        fn path_dirs(&mut self) -> Vec<PathBuf> {
            self.path.clone()
        }
        fn run(
            &mut self,
            program: &Path,
            args: &[String],
            timeout: Duration,
        ) -> Result<Ran, String> {
            self.calls
                .push((program.to_path_buf(), args.to_vec(), timeout));
            self.by_program
                .iter()
                .find(|(path, _)| path == program)
                .map_or_else(|| self.result.clone(), |(_, result)| result.clone())
        }
    }

    #[test]
    fn registered_runs_ignore_rule_for_both_executables() {
        let mut fake = Fake::running();
        let outcome = ensure_with(&mut fake, "komorebi-recorder.exe", true);
        assert_eq!(outcome, Outcome::Registered);
        assert_eq!(fake.calls.len(), 2);
        assert_eq!(fake.calls[0].0, PathBuf::from("C:/k/komorebic.exe"));
        assert_eq!(
            fake.calls[0].1,
            ["ignore-rule", "exe", "komorebi-recorder.exe"]
        );
        assert_eq!(fake.calls[1].1, ["ignore-rule", "exe", GUI_EXE]);
        assert_eq!(fake.calls[0].2, TIMEOUT);
    }

    #[test]
    fn the_gui_executable_is_not_registered_twice() {
        assert_eq!(exe_names("komorebi-recorder-gui.exe"), [GUI_EXE]);
        assert_eq!(
            exe_names("komorebi-recorder.exe"),
            ["komorebi-recorder.exe", GUI_EXE]
        );
    }

    #[test]
    fn not_running_runs_nothing() {
        let mut fake = Fake::running();
        fake.komorebi = None;
        assert_eq!(
            ensure_with(&mut fake, "a.exe", true),
            Outcome::KomorebiNotRunning
        );
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn komorebic_is_looked_for_next_to_komorebi_then_on_path() {
        let mut fake = Fake::running();
        fake.files = vec![PathBuf::from("D:/bin/komorebic.exe")];
        fake.path = vec![PathBuf::from("D:/bin")];
        assert_eq!(
            check(&mut fake, true),
            Readiness::Ready(PathBuf::from("D:/bin/komorebic.exe"))
        );

        let mut sibling = Fake::running();
        sibling.files.push(PathBuf::from("D:/bin/komorebic.exe"));
        sibling.path = vec![PathBuf::from("D:/bin")];
        assert_eq!(
            check(&mut sibling, true),
            Readiness::Ready(PathBuf::from("C:/k/komorebic.exe"))
        );
    }

    #[test]
    fn an_unknown_komorebi_binary_falls_back_to_path() {
        let mut fake = Fake::running();
        fake.komorebi = Some(None);
        fake.files = vec![PathBuf::from("D:/bin/komorebic.exe")];
        fake.path = vec![PathBuf::from("D:/bin")];
        assert!(matches!(check(&mut fake, true), Readiness::Ready(_)));
    }

    #[test]
    fn komorebic_missing() {
        let mut fake = Fake::running();
        fake.files.clear();
        assert_eq!(
            ensure_with(&mut fake, "a.exe", true),
            Outcome::KomorebicNotFound
        );
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn a_failing_command_is_failed() {
        let mut fake = Fake::running();
        fake.result = Ok(Ran::Exited(1));
        let outcome = ensure_with(&mut fake, "a.exe", true);
        assert!(matches!(outcome, Outcome::Failed(m) if m.contains("exited with 1")));
        assert_eq!(fake.calls.len(), 1, "stops at the first failure");
    }

    #[test]
    fn a_crashing_komorebic_next_to_komorebi_falls_back_to_the_one_on_path() {
        // a debug komorebic beside a dev komorebi can overflow its stack on every command
        let mut fake = Fake::running();
        fake.files.push(PathBuf::from("D:/bin/komorebic.exe"));
        fake.path = vec![PathBuf::from("D:/bin")];
        fake.by_program = vec![(
            PathBuf::from("C:/k/komorebic.exe"),
            Ok(Ran::Exited(-1_073_741_571)),
        )];
        assert_eq!(
            ensure_with(&mut fake, "komorebi-recorder.exe", true),
            Outcome::Registered
        );
        let programs: Vec<_> = fake.calls.iter().map(|call| call.0.clone()).collect();
        assert_eq!(
            programs,
            [
                PathBuf::from("C:/k/komorebic.exe"),
                PathBuf::from("D:/bin/komorebic.exe"),
                PathBuf::from("D:/bin/komorebic.exe"),
            ]
        );
    }

    #[test]
    fn when_every_komorebic_fails_the_failure_names_each_and_how() {
        let mut fake = Fake::running();
        fake.files.push(PathBuf::from("D:/bin/komorebic.exe"));
        fake.path = vec![PathBuf::from("D:/bin")];
        fake.by_program = vec![
            (
                PathBuf::from("C:/k/komorebic.exe"),
                Ok(Ran::Exited(-1_073_741_571)),
            ),
            (PathBuf::from("D:/bin/komorebic.exe"), Ok(Ran::Exited(1))),
        ];
        let Outcome::Failed(why) = ensure_with(&mut fake, "a.exe", true) else {
            panic!("expected a failure");
        };
        assert!(why.contains("C:/k"), "{why}");
        assert!(why.contains("overflowed its stack"), "{why}");
        assert!(why.contains("D:/bin"), "{why}");
        assert!(why.contains("exited with 1"), "{why}");
    }

    #[test]
    fn the_same_komorebic_is_not_tried_twice() {
        let mut fake = Fake::running();
        fake.path = vec![PathBuf::from("C:/k")];
        fake.result = Ok(Ran::Exited(1));
        let _ = ensure_with(&mut fake, "a.exe", true);
        assert_eq!(fake.calls.len(), 1, "one komorebic, found twice, run once");
    }

    #[test]
    fn exit_codes_are_read_as_answers_or_crashes() {
        assert_eq!(describe_exit(0), "exited with 0");
        assert_eq!(describe_exit(1), "exited with 1");
        assert!(describe_exit(-1_073_741_571).contains("0xc00000fd"));
        assert!(describe_exit(-1_073_741_571).contains("overflowed its stack"));
        assert!(describe_exit(-1_073_741_515).contains("DLL"));
        assert_eq!(describe_exit(-2), "crashed with status 0xfffffffe");
    }

    #[test]
    fn a_timeout_is_failed() {
        let mut fake = Fake::running();
        fake.result = Ok(Ran::TimedOut);
        let outcome = ensure_with(&mut fake, "a.exe", true);
        assert!(matches!(outcome, Outcome::Failed(m) if m.contains("3 seconds")));
    }

    #[test]
    fn a_command_that_cannot_start_is_failed() {
        let mut fake = Fake::running();
        fake.result = Err("denied".into());
        let outcome = ensure_with(&mut fake, "a.exe", true);
        assert!(matches!(outcome, Outcome::Failed(m) if m.contains("denied")));
    }

    #[test]
    fn not_windows_is_not_applicable_and_runs_nothing() {
        let mut fake = Fake::running();
        assert_eq!(
            ensure_with(&mut fake, "komorebi-recorder", false),
            Outcome::NotApplicable
        );
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn the_title_policy() {
        assert_eq!(title_for(&Outcome::Registered, true), TITLE);
        assert_eq!(title_for(&Outcome::KomorebiNotRunning, true), TITLE);
        assert_eq!(title_for(&Outcome::KomorebicNotFound, true), "");
        assert_eq!(title_for(&Outcome::Failed("x".into()), true), "");
        assert_eq!(title_for(&Outcome::NotApplicable, true), "");
        // never titled off Windows, whatever the outcome
        assert_eq!(title_for(&Outcome::Registered, false), "");
        assert_eq!(title_for(&Outcome::KomorebiNotRunning, false), "");
    }

    #[test]
    fn only_a_failure_to_register_has_a_note() {
        assert!(note(&Outcome::Registered, "a.exe").is_none());
        assert!(note(&Outcome::KomorebiNotRunning, "a.exe").is_none());
        assert!(note(&Outcome::NotApplicable, "a.exe").is_none());
        let text = note(&Outcome::KomorebicNotFound, "a.exe").unwrap();
        assert!(
            text.contains("komorebic not found")
                && text.ends_with("komorebic ignore-rule exe a.exe")
        );
        assert!(
            note(&Outcome::Failed("boom".into()), "a.exe")
                .unwrap()
                .contains("boom")
        );
    }

    #[test]
    fn doctor_describes_each_readiness() {
        assert!(describe(&Readiness::Ready("C:/k/komorebic.exe".into())).starts_with("titled"));
        assert!(describe(&Readiness::KomorebiNotRunning).starts_with("titled"));
        assert!(describe(&Readiness::KomorebicNotFound).starts_with("untitled"));
        assert!(describe(&Readiness::NotApplicable).starts_with("untitled"));
    }

    #[test]
    fn the_rule_text_names_both_executables() {
        let text = rule_text();
        assert!(text.contains("komorebi-recorder.exe") && text.contains(GUI_EXE));
    }
}
