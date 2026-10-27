//! Sending a rule to komorebi-application-specific-configuration as a pull request.
//!
//! [`submit`] does, with the tools the user already has and the credentials they have already set
//! up, what a person would do by hand:
//!
//! 1. clone the repository into a temporary directory (shallow: the rule file is all it needs);
//! 2. make a branch, and put the [`Rule`] in the platform's file ([`Rule::apply`]: only the lines
//!    of the rule change);
//! 3. commit it, with a message that says what the rule is and how and why it was found;
//! 4. push the branch and open a pull request, as below.
//!
//! # Pushing and opening the pull request
//!
//! With the GitHub CLI (`gh`) signed in, the branch goes to the repository itself if the user may
//! write to it, and to their fork of it (made if they have none) if not, and `gh pr create` opens
//! the pull request from there. The push is `git push`, so it uses whatever credential helper the
//! user has configured for git; `gh` is used for the rest, with its own sign-in.
//!
//! Without `gh`, or not signed in to it, the branch is pushed to the repository with `git push`:
//! that works for someone who may write to it, and the pull request is then [`Outcome::Pushed`], a
//! link for the user to open it with. For anyone else it fails, as does any step that fails, and
//! then the outcome is [`Outcome::Failed`]: it keeps the clone, with the commit in it, and says
//! what is left for the user to do to push it and open the pull request themselves.
//!
//! Nothing here asks a question: git is told not to prompt on a terminal (there is none), so a
//! credential helper with a window of its own can still ask, and anything that would otherwise
//! wait for a keyboard fails instead.
//!
//! # Tests
//!
//! Where the repository and the forks are is in [`Upstream`], so that the tests send to local
//! repositories and never to GitHub.

use super::Platform;
use super::rule::Applied;
use super::rule::Rule;
use crate::analysis::ghost::GhostTile;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

/// How long any one git or `gh` command may take: a clone of a repository this small, or a push,
/// is seconds, and a credential helper's window is waiting for a person.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(180);

/// How many times the push to a fork just made is tried: GitHub takes a moment to have one.
const FORK_PUSH_TRIES: usize = 4;

/// Where the rules go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    /// What `git clone` is given.
    pub url: String,
    /// The repository on GitHub, as `owner/name`.
    pub slug: String,
    /// The branch a pull request is for, and the clone is of.
    pub base: String,
    /// Where on the web a fork is: its address is `<fork_base>/<login>/<name>`.
    pub fork_base: String,
}

impl Upstream {
    /// komorebi-application-specific-configuration, on GitHub.
    #[must_use]
    pub fn komorebi_asc() -> Self {
        Self {
            url: "https://github.com/LGUG2Z/komorebi-application-specific-configuration.git"
                .to_string(),
            slug: "LGUG2Z/komorebi-application-specific-configuration".to_string(),
            base: "master".to_string(),
            fork_base: "https://github.com".to_string(),
        }
    }

    /// The repository's name without its owner.
    fn name(&self) -> &str {
        self.slug.rsplit('/').next().unwrap_or(&self.slug)
    }

    /// The address of a fork of the repository by `login`.
    fn fork_url(&self, login: &str) -> String {
        format!("{}/{login}/{}.git", self.fork_base, self.name())
    }

    /// The web page that starts a pull request of `branch` of `owner`'s copy.
    fn compare_url(&self, owner: &str, branch: &str) -> String {
        format!(
            "https://github.com/{}/compare/{}...{owner}:{branch}?expand=1",
            self.slug, self.base
        )
    }
}

/// The programs it works with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    /// `git`.
    pub git: OsString,
    /// The GitHub CLI, if there is one.
    pub gh: Option<OsString>,
    /// Environment variables for every command: none, unless something has to be told to them
    /// (the tests tell git who is committing).
    pub env: Vec<(String, String)>,
}

impl Tools {
    /// Looks for `gh` on the `PATH`, and where Homebrew puts it on a Mac, which a program started
    /// from Finder has no `PATH` to find: `git` is needed and not looked for, so that its absence
    /// says so when it is run.
    #[must_use]
    pub fn detect() -> Self {
        let gh = ["gh", "/opt/homebrew/bin/gh", "/usr/local/bin/gh"]
            .into_iter()
            .map(OsString::from)
            .find(|candidate| {
                run(
                    candidate,
                    &["--version"],
                    None,
                    &[],
                    Duration::from_secs(10),
                )
                .is_ok_and(|out| out.ok)
            });
        Self {
            git: OsString::from("git"),
            gh,
            env: Vec::new(),
        }
    }
}

/// What to send, and what to say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// Which file it goes in.
    pub platform: Platform,
    /// The rule.
    pub rule: Rule,
    /// The commit's first line and the pull request's title.
    pub title: String,
    /// The rest of the commit message and the pull request's description.
    pub body: String,
}

impl Proposal {
    /// The proposal that ignores a ghost tile: what was seen, why an ignore rule and why one this
    /// narrow, and how it was found, in words for the person who reads the pull request and has to
    /// decide. It is for the rule file of the komorebi the ghost was seen on.
    #[must_use]
    pub fn ghost_tile(ghost: &GhostTile, rule: Rule, version: &str) -> Self {
        let platform = ghost.platform;
        let title = format!("feat(apps): ignore the ghost window of {}", ghost.exe);
        let what = match platform {
            Platform::Windows => format!(
                "window {hwnd} of `{exe}` (class `{class}`, {titled})",
                hwnd = ghost.hwnd,
                exe = ghost.exe,
                class = ghost.class,
                titled = quoted_title(&ghost.title),
            ),
            Platform::Macos => format!(
                "window {hwnd} of `{exe}` ({titled})",
                hwnd = ghost.hwnd,
                exe = ghost.exe,
                titled = quoted_title(&ghost.title),
            ),
        };
        let evidence = match platform {
            Platform::Windows => format!(
                "on every one of those reads Windows said that the window was {how}. Windows never \
                 said it was on screen, and never that it was minimized, so this is not a window in \
                 the middle of appearing or one a person put away",
                how = ghost.hidden_how
            ),
            Platform::Macos => format!(
                "in every one of those states the frame of the window, read from the accessibility \
                 API, was {how}, and it was never a frame a person could see. (macOS cannot be asked \
                 whether a window is drawn, so this is the frame komorebi had for it)",
                how = ghost.hidden_how
            ),
        };
        let recorded_with = match platform {
            Platform::Windows => "with `komorebi-recorder record --probe-windows`",
            Platform::Macos => "of komorebi for Mac",
        };
        let reads = match platform {
            Platform::Windows => "read the state of",
            Platform::Macos => "recorded the state of",
        };
        let others = if ghost.siblings.is_empty() {
            format!(
                "The recording saw no other window of `{}`, but the application is more than this \
                 window, so the rule is a composite one all the same.",
                ghost.exe
            )
        } else {
            let mut seen: Vec<String> = ghost
                .siblings
                .iter()
                .map(|sibling| match platform {
                    Platform::Windows => format!(
                        "class `{}`, {}",
                        sibling.class,
                        quoted_title(&sibling.title)
                    ),
                    Platform::Macos => quoted_title(&sibling.title),
                })
                .collect();
            seen.sort();
            seen.dedup();
            format!(
                "`{exe}` has windows that are on screen and that this rule must not ignore: {list}.",
                exe = ghost.exe,
                list = seen.join("; ")
            )
        };
        let matched = match platform {
            Platform::Windows => {
                if rule
                    .identifiers
                    .iter()
                    .any(|identifier| identifier.kind == "Title")
                {
                    "the program, the class **and** the title: the program's other windows have \
                     this class too, and the title is what tells the ghost from them"
                } else {
                    "the program **and** the class"
                }
            }
            Platform::Macos => "the application **and** the title",
        };
        let body = format!(
            "Ignore {what}: {komorebi} gives it a tile that a person can never see.\n\
             \n\
             ## What happened\n\
             \n\
             A recording {recorded_with} {reads} {what} {reads_n} times between {first:.1} s and \
             {last:.1} s of its video. komorebi had it in front on {place}, so it took a tile of the \
             layout, and {evidence}: the tile stays empty for as long as the window exists{lasting}.\n\
             \n\
             ## Why this rule\n\
             \n\
             The README of this repository asks for `ignore` rules for ghost windows. This one \
             is a composite rule that matches {matched}, so that only this window is ignored and not \
             the application. {others}\n\
             \n\
             ## The rule\n\
             \n\
             For `{file}`:\n\
             \n\
             ```json\n{preview}\n```\n\
             \n\
             ## How it was added\n\
             \n\
             Proposed by komorebi-recorder {version}. The entry is put where it sorts and the rest \
             of the file is unchanged, line for line. `just sort` was not run.\n\
             \n\
             Please check, as with any ignore rule, that this window is not one that people do want \
             tiled.\n",
            komorebi = platform.komorebi(),
            reads_n = ghost.reads,
            first = ghost.first_video_s,
            last = ghost.last_video_s,
            place = ghost.place,
            lasting = if ghost.persisted_to_end {
                ", and it was still not on screen at the last read"
            } else {
                ""
            },
            file = platform.file(),
            preview = rule.preview(),
        );
        Self {
            platform,
            rule,
            title,
            body,
        }
    }
}

/// A title as a sentence has it: in quotes, or that there is none.
fn quoted_title(title: &str) -> String {
    if title.is_empty() {
        "no title".to_string()
    } else {
        format!("titled {title:?}")
    }
}
/// What [`submit`] came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The repository has the rule already: there is nothing to send.
    AlreadyThere,
    /// The pull request is open.
    Opened {
        /// Where.
        url: String,
        /// The branch it is from.
        branch: String,
    },
    /// The branch is pushed to the repository, and the user is left to open the pull request.
    Pushed {
        /// The branch.
        branch: String,
        /// A link that starts the pull request.
        compare_url: String,
    },
    /// Something failed. What is left to do is in the failure.
    Failed(Failure),
}

/// Where it went wrong, and what the user can do instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// What was being done: `cloning the repository`, `pushing the branch`...
    pub step: String,
    /// What went wrong, in the tool's own words.
    pub error: String,
    /// The temporary directory, which has the clone and the text of the pull request in it. `None`
    /// if there was none to keep.
    pub dir: Option<PathBuf>,
    /// The branch the commit is on.
    pub branch: String,
    /// What the user does to finish, in steps.
    pub instructions: String,
}

/// A step that failed, before it is put together with where it left things.
struct StepError {
    step: &'static str,
    error: String,
}

type Step<T> = Result<T, StepError>;

fn fail<T>(step: &'static str, error: impl Into<String>) -> Step<T> {
    Err(StepError {
        step,
        error: error.into(),
    })
}

/// What a command printed, and whether it succeeded.
struct Out {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Out {
    /// What to tell the user it said: the error stream, or failing that the output.
    fn said(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        text.trim().to_string()
    }
}

/// Runs `program` and waits up to `timeout` for it.
///
/// Returns an error when it cannot be started (it is not there) or does not finish in time (and
/// is then stopped).
fn run(
    program: &OsStr,
    args: &[&str],
    dir: Option<&Path>,
    env: &[(String, String)],
    timeout: Duration,
) -> Result<Out, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // there is no terminal to ask anything on: a credential helper with a window may still
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command.envs(env.iter().map(|(name, value)| (name, value)));
    #[cfg(windows)]
    crate::platform::process::isolate(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not run {}: {error}", program.to_string_lossy()))?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
            text
        })
    };
    let stdout = read(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    let stderr = read(child.stderr.take().map(|pipe| Box::new(pipe) as _));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} took longer than {} s and was stopped",
                    program.to_string_lossy(),
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(error) => {
                return Err(format!(
                    "could not wait for {}: {error}",
                    program.to_string_lossy()
                ));
            }
        }
    };
    Ok(Out {
        ok: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// Everything one submission has and does.
struct Submission<'a> {
    tools: &'a Tools,
    upstream: &'a Upstream,
    proposal: &'a Proposal,
    root: PathBuf,
    repo: PathBuf,
    branch: String,
    progress: &'a mut dyn FnMut(&str),
    /// The commit was made: what the user does instead starts from it.
    committed: bool,
}

/// Returns the name of a branch for a rule, which is unique to the second so that a second
/// proposal of the same thing never meets the first in the user's fork.
fn branch_name(rule: &Rule, now: chrono::DateTime<chrono::Utc>) -> String {
    let slug: String = rule
        .app
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    format!(
        "ghost-tile/{}-{}",
        if slug.is_empty() { "window" } else { slug },
        now.format("%Y%m%d-%H%M%S")
    )
}

/// A directory in the temporary one that nothing else has: named for the time and the process, and
/// for how many were made before it, since two proposals in a second are two directories.
fn fresh_directory(now: chrono::DateTime<chrono::Utc>) -> PathBuf {
    static MADE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    loop {
        let root = std::env::temp_dir().join(format!(
            "komorebi-recorder-asc-{}-{}-{}",
            now.format("%Y%m%dT%H%M%S"),
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        // `create_dir` fails if it is there, so that this one is ours
        if std::fs::create_dir(&root).is_ok() {
            return root;
        }
    }
}

/// Sends `proposal` to `upstream`, as in the [module](self) documentation.
///
/// `progress` is told what is being done, one short line at a time, as each step starts. It runs on
/// the calling thread, which is blocked for as long as the steps take (up to a few minutes at the
/// worst), so call this off the UI thread.
///
/// A temporary directory is made for the clone and is removed again once the pull request is open
/// or there turns out to be nothing to send; in any other case it is left, and the outcome says
/// where.
pub fn submit(
    proposal: &Proposal,
    tools: &Tools,
    upstream: &Upstream,
    progress: &mut dyn FnMut(&str),
) -> Outcome {
    let now = chrono::Utc::now();
    let root = fresh_directory(now);
    let mut work = Submission {
        tools,
        upstream,
        proposal,
        repo: root.join("repo"),
        root,
        branch: branch_name(&proposal.rule, now),
        progress,
        committed: false,
    };

    match work.go() {
        Ok(outcome) => {
            if matches!(outcome, Outcome::Opened { .. } | Outcome::AlreadyThere) {
                let _ = std::fs::remove_dir_all(&work.root);
            }
            outcome
        }
        Err(error) => Outcome::Failed(work.failure(error)),
    }
}

impl Submission<'_> {
    fn say(&mut self, what: &str) {
        (self.progress)(what);
    }

    fn git(&self, args: &[&str]) -> Result<Out, String> {
        run(
            &self.tools.git,
            args,
            Some(&self.repo),
            &self.tools.env,
            COMMAND_TIMEOUT,
        )
    }

    /// A git command that has to succeed.
    fn git_ok(&self, step: &'static str, args: &[&str]) -> Step<Out> {
        match self.git(args) {
            Ok(out) if out.ok => Ok(out),
            Ok(out) => fail(step, out.said()),
            Err(error) => fail(step, error),
        }
    }

    fn go(&mut self) -> Step<Outcome> {
        let file = self.proposal.platform.file();
        std::fs::create_dir_all(&self.root)
            .or_else(|error| fail("making a temporary directory", error.to_string()))?;

        self.say("cloning the repository");
        let clone = run(
            &self.tools.git,
            &[
                "clone",
                "--quiet",
                "--depth",
                "1",
                "--branch",
                &self.upstream.base,
                &self.upstream.url,
                &self.repo.to_string_lossy(),
            ],
            Some(&self.root),
            &self.tools.env,
            COMMAND_TIMEOUT,
        );
        match clone {
            Ok(out) if out.ok => {}
            Ok(out) => return fail("cloning the repository", out.said()),
            Err(error) => return fail("cloning the repository", error),
        }

        self.say("adding the rule");
        let path = self.repo.join(file);
        let before = std::fs::read_to_string(&path)
            .or_else(|error| fail("adding the rule", format!("could not read {file}: {error}")))?;
        let after = match self.proposal.rule.apply(&before) {
            Ok(Applied::Changed(text)) => text,
            Ok(Applied::AlreadyThere) => return Ok(Outcome::AlreadyThere),
            Err(error) => return fail("adding the rule", error.to_string()),
        };
        self.git_ok(
            "making a branch",
            &["checkout", "--quiet", "-b", &self.branch],
        )?;
        std::fs::write(&path, after).or_else(|error| {
            fail(
                "adding the rule",
                format!("could not write {file}: {error}"),
            )
        })?;

        self.say("committing it");
        let message = self.root.join("commit-message.txt");
        std::fs::write(
            &message,
            format!("{}\n\n{}", self.proposal.title, self.proposal.body),
        )
        .or_else(|error| fail("committing", error.to_string()))?;
        std::fs::write(
            self.root.join("pull-request.md"),
            format!("{}\n\n{}", self.proposal.title, self.proposal.body),
        )
        .or_else(|error| fail("committing", error.to_string()))?;
        self.git_ok("committing", &["add", file])?;
        self.git_ok(
            "committing",
            &["commit", "--quiet", "--file", &message.to_string_lossy()],
        )?;
        self.committed = true;

        self.push_and_open()
    }

    /// Whether `gh` is there and signed in.
    fn gh(&self) -> Option<&OsStr> {
        let gh = self.tools.gh.as_deref()?;
        run(
            gh,
            &["auth", "status"],
            None,
            &self.tools.env,
            Duration::from_secs(30),
        )
        .ok()
        .filter(|out| out.ok)
        .map(|_| gh)
    }

    /// What `gh` printed for `args`, in the clone, or the failure of `step`.
    fn gh_text(&self, gh: &OsStr, step: &'static str, args: &[&str]) -> Step<String> {
        match run(gh, args, Some(&self.repo), &self.tools.env, COMMAND_TIMEOUT) {
            Ok(out) if out.ok => Ok(out.stdout.trim().to_string()),
            Ok(out) => fail(step, out.said()),
            Err(error) => fail(step, error),
        }
    }

    fn push_and_open(&mut self) -> Step<Outcome> {
        let Some(gh) = self.gh().map(OsStr::to_os_string) else {
            // no way to open a pull request from here: a person who may write to the repository
            // can still push, and be given the link
            self.say("pushing the branch");
            self.git_ok(
                "pushing the branch",
                &["push", "--quiet", "origin", &self.branch.clone()],
            )?;
            let owner = self
                .upstream
                .slug
                .split('/')
                .next()
                .unwrap_or_default()
                .to_string();
            return Ok(Outcome::Pushed {
                compare_url: self.upstream.compare_url(&owner, &self.branch),
                branch: self.branch.clone(),
            });
        };

        self.say("checking whether you may push to the repository");
        let may_push = self.gh_text(
            &gh,
            "checking your access",
            &[
                "api",
                &format!("repos/{}", self.upstream.slug),
                "--jq",
                ".permissions.push",
            ],
        )?;

        let branch = self.branch.clone();
        let head = if may_push == "true" {
            self.say("pushing the branch");
            self.git_ok(
                "pushing the branch",
                &["push", "--quiet", "origin", &branch],
            )?;
            branch.clone()
        } else {
            self.say("forking the repository");
            self.gh_text(
                &gh,
                "forking the repository",
                &[
                    "repo",
                    "fork",
                    &self.upstream.slug,
                    "--clone=false",
                    "--remote=false",
                ],
            )?;
            let login = self.gh_text(
                &gh,
                "asking who you are",
                &["api", "user", "--jq", ".login"],
            )?;
            self.git_ok(
                "adding your fork",
                &["remote", "add", "fork", &self.upstream.fork_url(&login)],
            )?;

            self.say("pushing the branch to your fork");
            let mut last = String::new();
            let mut pushed = false;
            for attempt in 0..FORK_PUSH_TRIES {
                if attempt > 0 {
                    std::thread::sleep(Duration::from_secs(3));
                }
                match self.git(&["push", "--quiet", "fork", &branch]) {
                    Ok(out) if out.ok => {
                        pushed = true;
                        break;
                    }
                    Ok(out) => last = out.said(),
                    Err(error) => last = error,
                }
            }
            if !pushed {
                return fail("pushing the branch to your fork", last);
            }
            format!("{login}:{branch}")
        };

        self.say("opening the pull request");
        let body = self.root.join("pull-request-body.md");
        std::fs::write(&body, &self.proposal.body)
            .or_else(|error| fail("opening the pull request", error.to_string()))?;
        let output = self.gh_text(
            &gh,
            "opening the pull request",
            &[
                "pr",
                "create",
                "--repo",
                &self.upstream.slug,
                "--base",
                &self.upstream.base,
                "--head",
                &head,
                "--title",
                &self.proposal.title,
                "--body-file",
                &body.to_string_lossy(),
            ],
        )?;
        let url = output
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| line.starts_with("http"))
            .map_or_else(|| output.clone(), str::to_string);
        Ok(Outcome::Opened { url, branch })
    }

    /// The failure, with what the user can do about it.
    fn failure(&self, error: StepError) -> Failure {
        let dir = self.root.is_dir().then(|| self.root.clone());
        let instructions = self.instructions(&error);
        Failure {
            step: error.step.to_string(),
            error: error.error,
            dir,
            branch: self.branch.clone(),
            instructions,
        }
    }

    fn instructions(&self, error: &StepError) -> String {
        let up = self.upstream;
        let file = self.proposal.platform.file();
        if !self.committed {
            return format!(
                "The rule could not be prepared ({}). To propose it by hand, add this to {file} in \
                 https://github.com/{}, in its place in the sorted list, and open a pull request:\n\n{}\n",
                error.step,
                up.slug,
                self.proposal.rule.preview()
            );
        }
        let repo = self.repo.display();
        let pull_request = self.root.join("pull-request.md");
        format!(
            "The rule is committed on the branch {branch} of the clone in\n  {repo}\n\
             To open the pull request yourself:\n\
             \n\
             1. Fork https://github.com/{slug} if you have not (https://github.com/{slug}/fork).\n\
             2. In the clone, add your fork and push the branch to it:\n\
             \n\
             \x20    cd \"{repo}\"\n\
             \x20    git remote add fork https://github.com/<you>/{name}.git\n\
             \x20    git push -u fork {branch}\n\
             \n\
             3. Open https://github.com/{slug}/compare/{base}...<you>:{branch}?expand=1 and use \
             the title and description in\n  {pr}\n",
            branch = self.branch,
            slug = up.slug,
            name = up.name(),
            base = up.base,
            pr = pull_request.display(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::ghost::Sibling;
    use crate::asc::rule::Identifier;

    fn git_available() -> bool {
        run(
            OsStr::new("git"),
            &["--version"],
            None,
            &[],
            Duration::from_secs(10),
        )
        .is_ok_and(|out| out.ok)
    }

    const FILE: &str = "{\n  \"$schema\": \"https://example.com/schema.json\",\n  \"Zoom\": {\n    \"floating\": [\n      {\n        \"kind\": \"Class\",\n        \"id\": \"ZPContentViewWndClass\",\n        \"matching_strategy\": \"Equals\"\n      }\n    ],\n    \"ignore\": [\n      {\n        \"kind\": \"Class\",\n        \"id\": \"ZoomGhostClass\",\n        \"matching_strategy\": \"Equals\"\n      }\n    ]\n  }\n}\n";

    /// The Mac file: its own layout of the same kind.
    const MAC_FILE: &str = "{\n  \"$schema\": \"https://example.com/mac.json\",\n  \"Discord\": {\n    \"ignore\": [\n      [\n        {\n          \"kind\": \"Exe\",\n          \"id\": \"Discord\",\n          \"matching_strategy\": \"Equals\"\n        },\n        {\n          \"kind\": \"Title\",\n          \"id\": \"DiscordUpdater\",\n          \"matching_strategy\": \"Equals\"\n        }\n      ]\n    ]\n  }\n}\n";

    fn rule() -> Rule {
        Rule::ignore(
            "DDM",
            vec![Identifier::exe("DDM.exe"), Identifier::class("DDMWindow")],
        )
    }

    fn proposal() -> Proposal {
        Proposal {
            platform: Platform::Windows,
            rule: rule(),
            title: "feat(apps): ignore the ghost window of DDM.exe".to_string(),
            body: "Why and how.\n".to_string(),
        }
    }

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = run(
            OsStr::new("git"),
            args,
            Some(dir),
            &identity_env(),
            Duration::from_secs(30),
        )
        .expect("git");
        assert!(out.ok, "git {args:?}: {}", out.said());
        out.stdout.trim().to_string()
    }

    /// A bare repository with the rule file in it, on `master`, and the place to put it.
    struct Fixture {
        base: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "komorebi-recorder-asc-test-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&base);
            let seed = base.join("seed");
            std::fs::create_dir_all(&seed).expect("seed");
            git_in(&seed, &["init", "--quiet", "--initial-branch=master"]);
            std::fs::write(seed.join("applications.json"), FILE).expect("file");
            std::fs::write(seed.join("applications.mac.json"), MAC_FILE).expect("mac file");
            git_in(&seed, &["add", "."]);
            git_in(
                &seed,
                &[
                    "-c",
                    "user.name=T",
                    "-c",
                    "user.email=t@example.com",
                    "commit",
                    "--quiet",
                    "-m",
                    "seed",
                ],
            );
            let bare = base.join("asc.git");
            git_in(&base, &["clone", "--quiet", "--bare", "seed", "asc.git"]);
            assert!(bare.is_dir());
            Self { base }
        }

        fn upstream(&self) -> Upstream {
            Upstream {
                url: self.base.join("asc.git").to_string_lossy().into_owned(),
                slug: "LGUG2Z/komorebi-application-specific-configuration".to_string(),
                base: "master".to_string(),
                // forks are directories of bare repositories here
                fork_base: self.base.join("forks").to_string_lossy().into_owned(),
            }
        }

        fn bare(&self) -> PathBuf {
            self.base.join("asc.git")
        }

        /// A fake `gh` that answers as a signed-in user who `may_push` or not, and says a pull
        /// request was opened.
        fn gh(&self, may_push: bool) -> OsString {
            #[cfg(windows)]
            let (name, script) = (
                "gh.cmd",
                format!(
                    "@echo off\r\n\
                     if \"%1\"==\"auth\" exit /b 0\r\n\
                     if \"%1\"==\"repo\" exit /b 0\r\n\
                     if \"%1\"==\"api\" if \"%2\"==\"user\" (echo someone& exit /b 0)\r\n\
                     if \"%1\"==\"api\" (echo {may_push}& exit /b 0)\r\n\
                     if \"%1\"==\"pr\" (echo https://github.com/LGUG2Z/x/pull/7& exit /b 0)\r\n\
                     exit /b 1\r\n"
                ),
            );
            #[cfg(not(windows))]
            let (name, script) = (
                "gh.sh",
                format!(
                    "#!/bin/sh\n\
                     case \"$1\" in\n\
                     auth|repo) exit 0;;\n\
                     api) if [ \"$2\" = user ]; then echo someone; else echo {may_push}; fi; exit 0;;\n\
                     pr) echo https://github.com/LGUG2Z/x/pull/7; exit 0;;\n\
                     esac\n\
                     exit 1\n"
                ),
            );
            let path = self.base.join(name);
            std::fs::write(&path, script).expect("fake gh");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod");
            }
            path.into_os_string()
        }

        /// A bare repository for `someone`'s fork.
        fn fork(&self) {
            let forks = self.base.join("forks").join("someone");
            std::fs::create_dir_all(&forks).expect("forks");
            git_in(
                &forks,
                &[
                    "clone",
                    "--quiet",
                    "--bare",
                    &self.bare().to_string_lossy(),
                    "komorebi-application-specific-configuration.git",
                ],
            );
        }
    }

    /// Who commits: a machine under test may have no git identity of its own.
    fn identity_env() -> Vec<(String, String)> {
        [
            ("GIT_AUTHOR_NAME", "Test"),
            ("GIT_AUTHOR_EMAIL", "test@example.com"),
            ("GIT_COMMITTER_NAME", "Test"),
            ("GIT_COMMITTER_EMAIL", "test@example.com"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
    }

    fn tools(gh: Option<OsString>) -> Tools {
        Tools {
            git: "git".into(),
            gh,
            env: identity_env(),
        }
    }

    fn submit_with(tools: &Tools, upstream: &Upstream) -> (Outcome, Vec<String>) {
        let mut said = Vec::new();
        let outcome = submit(&proposal(), tools, upstream, &mut |line| {
            said.push(line.to_string())
        });
        (outcome, said)
    }

    #[test]
    fn a_branch_is_named_for_the_application_and_the_second() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-27T11:45:01Z")
            .expect("time")
            .with_timezone(&chrono::Utc);

        assert_eq!(branch_name(&rule(), now), "ghost-tile/ddm-20261027-114501");
        assert_eq!(
            branch_name(&Rule::ignore("Windows Terminal!", vec![]), now),
            "ghost-tile/windows-terminal-20261027-114501"
        );
        assert_eq!(
            branch_name(&Rule::ignore("***", vec![]), now),
            "ghost-tile/window-20261027-114501"
        );
    }

    #[test]
    fn the_pull_request_says_what_was_seen_and_why() {
        let ghost = GhostTile {
            platform: Platform::Windows,
            hwnd: 131_072,
            exe: "DDM.exe".into(),
            class: "DDMWindow".into(),
            title: String::new(),
            hidden_how: "cloaked".into(),
            place: "monitor 65537 workspace one (container 0)".into(),
            reads: 3,
            first_seq: 7,
            first_video_s: 2.0,
            last_video_s: 9.5,
            persisted_to_end: true,
            siblings: vec![Sibling {
                id: 9,
                class: "DDMMain".into(),
                title: "Display Manager".into(),
            }],
        };
        let rule = Rule::for_ghost(&ghost).expect("rule");

        let proposal = Proposal::ghost_tile(&ghost, rule, "0.1.1");

        assert_eq!(
            proposal.title,
            "feat(apps): ignore the ghost window of DDM.exe"
        );
        for expected in [
            "window 131072 of `DDM.exe` (class `DDMWindow`, no title)",
            "komorebi for Windows",
            "3 times",
            "cloaked",
            "never said it was on screen",
            "the program **and** the class",
            "windows that are on screen and that this rule must not ignore",
            "class `DDMMain`, titled \"Display Manager\"",
            "For `applications.json`",
            "\"id\": \"DDMWindow\"",
            "komorebi-recorder 0.1.1",
            "not on screen at the last read",
        ] {
            assert!(
                proposal.body.contains(expected),
                "{expected}: {}",
                proposal.body
            );
        }
    }

    #[test]
    fn without_gh_a_branch_that_can_be_pushed_is_pushed_and_a_link_is_given() {
        if !git_available() {
            eprintln!("skipped: git is not available");
            return;
        }
        let fixture = Fixture::new("pushed");

        let (outcome, said) = submit_with(&tools(None), &fixture.upstream());

        let Outcome::Pushed {
            branch,
            compare_url,
        } = outcome
        else {
            panic!("{outcome:#?}");
        };
        assert!(branch.starts_with("ghost-tile/ddm-"), "{branch}");
        assert!(compare_url.contains("/compare/master..."), "{compare_url}");
        assert_eq!(
            said.first().map(String::as_str),
            Some("cloning the repository")
        );

        // the branch is in the repository, with one commit that changed only the rule file
        let files = git_in(&fixture.bare(), &["diff", "--name-only", "master", &branch]);
        assert_eq!(files, "applications.json");
        let changed = git_in(&fixture.bare(), &["diff", "master", &branch]);
        assert!(changed.contains("+  \"DDM\": {"), "{changed}");
        assert_eq!(
            changed
                .lines()
                .filter(|line| line.starts_with('-') && !line.starts_with("---"))
                .count(),
            0,
            "an entry that sorts before another is added without touching any line: {changed}"
        );
        let subject = git_in(&fixture.bare(), &["log", "-1", "--format=%s", &branch]);
        assert_eq!(subject, "feat(apps): ignore the ghost window of DDM.exe");
    }

    #[test]
    fn a_signed_in_gh_opens_the_pull_request_from_the_repository_when_pushing_is_allowed() {
        if !git_available() {
            eprintln!("skipped: git is not available");
            return;
        }
        let fixture = Fixture::new("opened");
        let tools = tools(Some(fixture.gh(true)));

        let (outcome, said) = submit_with(&tools, &fixture.upstream());

        let Outcome::Opened { url, branch } = outcome else {
            panic!("{outcome:#?}");
        };
        assert_eq!(url, "https://github.com/LGUG2Z/x/pull/7");
        assert!(
            said.contains(&"opening the pull request".to_string()),
            "{said:?}"
        );
        git_in(&fixture.bare(), &["rev-parse", "--verify", &branch]);
    }

    #[test]
    fn without_write_access_the_branch_goes_to_a_fork_the_user_has() {
        if !git_available() {
            eprintln!("skipped: git is not available");
            return;
        }
        let fixture = Fixture::new("forked");
        fixture.fork();
        let tools = tools(Some(fixture.gh(false)));

        let (outcome, said) = submit_with(&tools, &fixture.upstream());

        let Outcome::Opened { branch, .. } = outcome else {
            panic!("{outcome:#?} {said:?}");
        };
        assert!(
            said.contains(&"forking the repository".to_string()),
            "{said:?}"
        );
        let fork = fixture
            .base
            .join("forks")
            .join("someone")
            .join("komorebi-application-specific-configuration.git");
        git_in(&fork, &["rev-parse", "--verify", &branch]);
        // and the repository itself was not pushed to
        let heads = git_in(&fixture.bare(), &["branch", "--list"]);
        assert!(!heads.contains("ghost-tile"), "{heads}");
    }

    #[test]
    fn a_push_that_is_refused_leaves_the_clone_and_says_what_to_do() {
        if !git_available() {
            eprintln!("skipped: git is not available");
            return;
        }
        let fixture = Fixture::new("refused");
        let hook = fixture.bare().join("hooks").join("pre-receive");
        std::fs::write(&hook, "#!/bin/sh\necho 'no pushes here' >&2\nexit 1\n").expect("hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }

        let (outcome, _) = submit_with(&tools(None), &fixture.upstream());

        let Outcome::Failed(failure) = outcome else {
            panic!("{outcome:#?}");
        };
        assert_eq!(failure.step, "pushing the branch");
        assert!(
            failure.error.contains("no pushes here"),
            "{}",
            failure.error
        );
        let dir = failure.dir.expect("the clone is kept");
        // the commit is in it, and what is left to do names it
        let subject = git_in(&dir.join("repo"), &["log", "-1", "--format=%s"]);
        assert_eq!(subject, "feat(apps): ignore the ghost window of DDM.exe");
        assert!(dir.join("pull-request.md").is_file());
        for expected in [
            &failure.branch,
            "git remote add fork",
            "/fork",
            "compare/master...<you>:",
            "pull-request.md",
        ] {
            assert!(
                failure.instructions.contains(expected),
                "{expected}: {}",
                failure.instructions
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_repository_that_cannot_be_cloned_says_so_and_gives_the_rule_to_send_by_hand() {
        let mut upstream = Fixture::new("noclone").upstream();
        upstream.url = std::env::temp_dir()
            .join("komorebi-recorder-asc-test-nothing-here")
            .to_string_lossy()
            .into_owned();
        if !git_available() {
            eprintln!("skipped: git is not available");
            return;
        }

        let (outcome, _) = submit_with(&tools(None), &upstream);

        let Outcome::Failed(failure) = outcome else {
            panic!("{outcome:#?}");
        };
        assert_eq!(failure.step, "cloning the repository");
        assert!(
            failure.instructions.contains("\"DDMWindow\""),
            "{}",
            failure.instructions
        );
        if let Some(dir) = failure.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_rule_the_repository_has_already_is_not_sent_again() {
        if !git_available() {
            eprintln!("skipped: git is not available");
            return;
        }
        let fixture = Fixture::new("there");
        let mut again = proposal();
        again.rule = Rule::ignore("Zoom", vec![Identifier::class("ZoomGhostClass")]);

        let mut said = Vec::new();
        let outcome = submit(&again, &tools(None), &fixture.upstream(), &mut |line| {
            said.push(line.to_string());
        });

        assert_eq!(outcome, Outcome::AlreadyThere, "{said:?}");
        // and nothing is left behind, or pushed
        let heads = git_in(&fixture.bare(), &["branch", "--list"]);
        assert!(!heads.contains("ghost-tile"), "{heads}");
    }

    fn mac_ghost() -> GhostTile {
        GhostTile {
            platform: Platform::Macos,
            hwnd: 7,
            exe: "Discord".into(),
            class: String::new(),
            title: "Rich Presence".into(),
            hidden_how: "zero-sized".into(),
            place: "monitor 1 workspace one (container 0)".into(),
            reads: 2,
            first_seq: 3,
            first_video_s: 1.0,
            last_video_s: 4.0,
            persisted_to_end: false,
            siblings: vec![Sibling {
                id: 8,
                class: String::new(),
                title: "General | Server".into(),
            }],
        }
    }

    #[test]
    fn a_mac_ghost_is_proposed_for_the_mac_file_in_the_words_of_a_mac() {
        let ghost = mac_ghost();
        let rule = Rule::for_ghost(&ghost).expect("rule");

        let proposal = Proposal::ghost_tile(&ghost, rule, "0.1.1");

        assert_eq!(proposal.platform, Platform::Macos);
        for expected in [
            "komorebi for Mac",
            "window 7 of `Discord` (titled \"Rich Presence\")",
            "accessibility API",
            "zero-sized",
            "the application **and** the title",
            "titled \"General | Server\"",
            "For `applications.mac.json`",
            "\"kind\": \"Title\"",
        ] {
            assert!(
                proposal.body.contains(expected),
                "{expected}: {}",
                proposal.body
            );
        }
        for unexpected in ["Windows said", "--probe-windows", "class `"] {
            assert!(
                !proposal.body.contains(unexpected),
                "{unexpected}: {}",
                proposal.body
            );
        }
    }

    #[test]
    fn a_mac_rule_goes_into_the_mac_file_and_the_windows_file_is_left_alone() {
        if !git_available() {
            eprintln!("skipped: git is not available");
            return;
        }
        let fixture = Fixture::new("mac");
        let ghost = mac_ghost();
        let proposal =
            Proposal::ghost_tile(&ghost, Rule::for_ghost(&ghost).expect("rule"), "0.1.1");

        let mut said = Vec::new();
        let outcome = submit(&proposal, &tools(None), &fixture.upstream(), &mut |line| {
            said.push(line.to_string());
        });

        let Outcome::Pushed { branch, .. } = outcome else {
            panic!("{outcome:#?} {said:?}");
        };
        let files = git_in(&fixture.bare(), &["diff", "--name-only", "master", &branch]);
        assert_eq!(files, "applications.mac.json");
        // Discord has an entry already: the rule joins its `ignore` list, after the one it has
        let text = git_in(
            &fixture.bare(),
            &["show", &format!("{branch}:applications.mac.json")],
        );
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        let rules = value["Discord"]["ignore"].as_array().expect("ignore");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1][0]["id"], "Discord");
        assert_eq!(rules[1][1]["kind"], "Title");
        assert_eq!(rules[1][1]["id"], "Rich Presence");
    }
}
