//! Which line of komorebi's source wrote a log line.
//!
//! komorebi's log records what happened but not where in its code, and a fix needs the code.
//! `build.rs` indexes every `tracing` statement in the source of each flavor of komorebi (see
//! [`super::flavor`]) while the recorder is built, and this module holds that index as a
//! [`Catalog`] per flavor. A recorded line is matched to the catalog's statements by module, level
//! and message: the message of a statement is a format string, so `reaping window: {window_id}`
//! matches the line `reaping window: 4711` ([`Catalog::find`]). `code` and `trace` use it, and so
//! does the viewer.
//!
//! A match is only as good as the source it was made against. The recording says which komorebi was
//! running (see [`crate::sources::komorebi_version`]) and the catalog records the commit it was
//! indexed at, so a mismatch is reported with every answer ([`Skew`]) instead of letting a line be
//! pinned on code that may not be what ran.
//!
//! # Where the index comes from
//!
//! There is one catalog per flavor, because komorebi for Mac and komorebi for Windows are different
//! programs with different logs, and [`of`] picks the one for the flavor a session recorded (see
//! [`Analysis::catalog`](super::Analysis::catalog)): a session recorded on Windows is mapped to
//! komorebi for Windows' source even when it is investigated on a Mac. Each is built from a
//! checkout of that komorebi named by an environment variable when the recorder is compiled:
//!
//! | flavor | checkout | its commit, outside a git checkout | directories indexed |
//! |---|---|---|---|
//! | [`Flavor::Macos`] | `KOMOREBI_MAC_SRC` | `KOMOREBI_MAC_COMMIT` | `komorebi/src`, `komorebi-client/src` |
//! | [`Flavor::Windows`] | `KOMOREBI_WINDOWS_SRC` | `KOMOREBI_WINDOWS_COMMIT` | `komorebi/src`, `komorebi-layouts/src` |
//!
//! `nix develop` sets all four. A flavor whose variable is not set (or whose checkout has no Rust
//! files in those directories) gets an **empty** catalog: nothing is found in it,
//! [`Catalog::is_empty`] is `true`, and [`skew`] tells the reader which variable to set. `build.rs`
//! prints a cargo warning if a checkout is named but nothing in it could be read, or if a file
//! fails to parse.
//!
//! What is indexed, for each Rust file of those directories:
//!
//! - every `tracing::error!`, `warn!`, `info!`, `debug!` and `trace!` call (also written as a bare
//!   `info!`), with its level, its target (the module path, such as `komorebi::reaper`, or the
//!   `target:` the call names), the first string literal argument as its template, the file
//!   (relative to the checkout, always `/`-separated), the line, the enclosing function and the
//!   type of its `impl`. A call inside another macro is found if the macro's body parses as items
//!   or statements, as the ones inside objc2's `define_class!` do;
//! - every `#[instrument]` function, as a span named after the function (or after its `name = `),
//!   which is what a log line's span chain refers to ([`Catalog::span`]).
//!
//! A statement the scan cannot see is missing from the index and a line from it is not found. For
//! komorebi for Mac, the unit test `every_log_statement_in_komorebis_source_is_indexed` fails when
//! the number of `tracing::<level>!` calls in its source differs from the number indexed.
//!
//! The generated code (`log_catalog.rs` in the build's output directory) is included below. It
//! defines the statics `MAC` and `WINDOWS` that [`of`] returns. The directory of each checkout is
//! also built into the recorder, so that `code` can find it again when it is run
//! ([`crate::annotations::default_code_root`]).

use super::flavor::Flavor;
use super::logparse::Level;
use crate::sources::komorebi_version::KomorebiVersion;
use schemars::JsonSchema;
use serde::Serialize;

/// A log statement as `build.rs` wrote it into the generated code. [`Statement`] is the owned form
/// that analysis hands out.
pub struct RawEntry {
    /// `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`.
    pub level: &'static str,
    /// The module the statement logs from, as komorebi's log shows it: `komorebi::reaper`.
    pub target: &'static str,
    /// The statement's format string, such as `reaping window: {window_id}`; empty if its message
    /// is not a string literal.
    pub template: &'static str,
    /// The file of the statement, relative to the checkout and `/`-separated.
    pub file: &'static str,
    /// The line the macro call starts on, counting from 1.
    pub line: usize,
    /// The function the statement is in; empty outside any function.
    pub function: &'static str,
    /// The type whose `impl` the function is in; empty if it is not in one.
    pub owner: &'static str,
}

/// An `#[instrument]`ed function, as `build.rs` wrote it into the generated code: the span its log
/// lines carry.
pub struct RawSpan {
    /// The span's name: the function's, unless `#[instrument(name = "...")]` renames it.
    pub name: &'static str,
    /// The file of the function, relative to the checkout and `/`-separated.
    pub file: &'static str,
    /// The line of the `#[instrument]` attribute.
    pub line: usize,
    /// The function the attribute is on.
    pub function: &'static str,
    /// The type whose `impl` the function is in; empty if it is not in one.
    pub owner: &'static str,
}

/// Every log statement and span of one flavor's komorebi source, as `build.rs` indexed them.
///
/// There is no way to make one: [`of`] returns the catalog built into the recorder for a
/// [`Flavor`], which can be empty (see the [module](self) documentation). It records the commit its
/// source was at and whether that source had uncommitted changes, so that [`skew`] can report
/// whether it describes the komorebi that was recorded.
///
/// The fields are private, so a catalog cannot be built by hand outside this module:
///
/// ```rust,compile_fail,E0451
/// use komorebi_recorder::analysis::catalog::Catalog;
///
/// let _ = Catalog {
///     commit: "",
///     dirty: false,
///     files_scanned: 0,
///     source_paths: &[],
///     entries: &[],
///     spans: &[],
/// };
/// ```
///
/// The one that exists for a flavor is returned by [`of`]:
///
/// ```rust
/// use komorebi_recorder::analysis::catalog;
/// use komorebi_recorder::analysis::flavor::Flavor;
///
/// let mac = catalog::of(Flavor::Macos);
/// if !mac.is_empty() {
///     // statements come from the files that were parsed
///     assert!(mac.files_scanned() > 0);
/// }
/// ```
pub struct Catalog {
    commit: &'static str,
    dirty: bool,
    files_scanned: usize,
    /// The directories of the checkout that were indexed.
    source_paths: &'static [&'static str],
    entries: &'static [RawEntry],
    spans: &'static [RawSpan],
}

include!(concat!(env!("OUT_DIR"), "/log_catalog.rs"));

/// Returns the catalog of the source of `flavor`'s komorebi, which is empty if the recorder was
/// built without that checkout.
#[must_use]
pub fn of(flavor: Flavor) -> &'static Catalog {
    match flavor {
        Flavor::Macos => &MAC,
        Flavor::Windows => &WINDOWS,
    }
}

impl Catalog {
    /// Returns the commit of the source that was indexed, or `None` if the checkout was not a git
    /// repository and the build was not told the commit either (`KOMOREBI_MAC_COMMIT` or
    /// `KOMOREBI_WINDOWS_COMMIT`), as is the case for an empty catalog.
    #[must_use]
    pub fn commit(&self) -> Option<&'static str> {
        (!self.commit.is_empty()).then_some(self.commit)
    }

    /// Returns whether the indexed directories of the checkout had uncommitted changes when it was
    /// indexed. This is `false` whenever the commit is unknown.
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Returns how many source files were parsed to build the catalog. A file that failed to parse
    /// is not counted.
    #[must_use]
    pub fn files_scanned(&self) -> usize {
        self.files_scanned
    }

    /// Returns how many log statements were indexed. `#[instrument]` spans are not counted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no log statement was indexed: the recorder was built without a checkout of
    /// this flavor's komorebi, or the checkout had none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// How far a [`Source`] can be trusted to be the statement that wrote a log line.
///
/// A log line is matched to statements by their text alone, so the confidence reports how much the
/// text pins down. It is never a statement about whether the source is the version that ran: that
/// is [`Skew`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    /// The message is the statement's whole text and nothing else matched.
    Exact,
    /// A statement with distinctive text matched, and nothing else did as well.
    Likely,
    /// Several statements matched equally well.
    Ambiguous,
    /// Only a statement made of placeholders (`"{}"`) matched, so it says little: at most three
    /// bytes of its text are literal.
    Generic,
}

/// A log statement of komorebi's source: the owned form of a [`RawEntry`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Statement {
    /// `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`. Empty for a span ([`Catalog::span`]).
    pub level: String,
    /// The module the statement logs from, as komorebi's log shows it: `komorebi::reaper`. Empty
    /// for a span.
    pub target: String,
    /// The statement's format string, e.g. `reaping window: {window_id}`. For a span it is
    /// `#[instrument]` and the span's name.
    pub template: String,
    /// Relative to the repository root, and always `/`-separated.
    pub file: String,
    /// The line of the statement in `file`, counting from 1.
    pub line: usize,
    /// The function the statement is in.
    pub function: String,
    /// The type whose `impl` it is in, if any.
    pub owner: String,
}

impl Statement {
    fn of(entry: &RawEntry) -> Self {
        Self {
            level: entry.level.to_string(),
            target: entry.target.to_string(),
            template: entry.template.to_string(),
            file: entry.file.to_string(),
            line: entry.line,
            function: entry.function.to_string(),
            owner: entry.owner.to_string(),
        }
    }

    /// Returns the location as `file:line`, as an editor opens it.
    #[must_use]
    pub fn location(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }

    /// Returns the function with the type of its `impl` in front, such as
    /// `WindowManager::process_event`, or the bare function name if it is not in an `impl`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::catalog::Statement;
    ///
    /// let statement = Statement {
    ///     level: "INFO".to_string(),
    ///     target: "komorebi::process_event".to_string(),
    ///     template: "processing event: {} for process {} with notification {}".to_string(),
    ///     file: "komorebi/src/process_event.rs".to_string(),
    ///     line: 120,
    ///     function: "process_event".to_string(),
    ///     owner: "WindowManager".to_string(),
    /// };
    /// assert_eq!(statement.location(), "komorebi/src/process_event.rs:120");
    /// assert_eq!(statement.qualified_function(), "WindowManager::process_event");
    ///
    /// // outside any `impl`, the function stands alone
    /// let free = Statement { owner: String::new(), ..statement };
    /// assert_eq!(free.qualified_function(), "process_event");
    /// ```
    #[must_use]
    pub fn qualified_function(&self) -> String {
        if self.owner.is_empty() {
            self.function.clone()
        } else {
            format!("{}::{}", self.owner, self.function)
        }
    }
}

/// The statement that wrote a log line, and how sure the match is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Source {
    /// The best match. When several matched equally well it is the first of them in the catalog's
    /// order: file by file, in source order within a file.
    pub statement: Statement,
    /// How much the text of the line pins the statement down.
    pub confidence: Confidence,
    /// Other statements that matched as well as this one: empty unless `confidence` is
    /// [`Confidence::Ambiguous`] or [`Confidence::Generic`].
    pub alternatives: Vec<Statement>,
}

/// The literal pieces of a format string around its placeholders: `a {} b {x:?} c` is
/// `["a ", " b ", " c"]`. `{{` and `}}` are literal braces.
fn literals(template: &str) -> Vec<String> {
    let mut pieces = vec![String::new()];
    let mut characters = template.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '{' if characters.peek() == Some(&'{') => {
                characters.next();
                pieces.last_mut().expect("never empty").push('{');
            }
            '}' if characters.peek() == Some(&'}') => {
                characters.next();
                pieces.last_mut().expect("never empty").push('}');
            }
            '{' => {
                for inner in characters.by_ref() {
                    if inner == '}' {
                        break;
                    }
                }
                pieces.push(String::new());
            }
            other => pieces.last_mut().expect("never empty").push(other),
        }
    }

    pieces
}

/// How much literal text of `template` `message` shares, or `None` if it cannot be an expansion of
/// it. The literal pieces must appear in order, the first at the start and the last at the end.
fn score(template: &str, message: &str) -> Option<usize> {
    let pieces = literals(template);
    if pieces.len() == 1 {
        return (pieces[0] == message).then(|| pieces[0].len() * 2 + 1);
    }

    let (first, last) = (&pieces[0], &pieces[pieces.len() - 1]);
    if !message.starts_with(first.as_str())
        || message.len() < first.len() + last.len()
        || !message.ends_with(last.as_str())
    {
        return None;
    }

    let mut rest = &message[first.len()..message.len() - last.len()];
    for piece in &pieces[1..pieces.len() - 1] {
        let at = rest.find(piece.as_str())?;
        rest = &rest[at + piece.len()..];
    }

    Some(pieces.iter().map(String::len).sum())
}

impl Catalog {
    /// Finds the statement that wrote a log line, or `None` if no statement could have.
    ///
    /// `target` is the module the line was logged from and `level` its level; both must be
    /// equal to the statement's. `message` is the line's message, which must be an expansion of the
    /// statement's template: the template's literal text has to appear in order, the first piece at
    /// the start and the last at the end, and each placeholder stands for any text. A template
    /// without placeholders must equal the message.
    ///
    /// Of the statements that match, the one sharing the most literal text with the message wins:
    /// its score is the number of bytes of the template's literal text, or twice that plus one for
    /// a template with no placeholders. The [`Confidence`] says how that went:
    ///
    /// | confidence | when |
    /// |---|---|
    /// | [`Confidence::Generic`] | the winner's score is 3 or less, whatever else matched |
    /// | [`Confidence::Ambiguous`] | otherwise, another statement shares exactly as much |
    /// | [`Confidence::Exact`] | otherwise, the winner has no placeholders |
    /// | [`Confidence::Likely`] | otherwise |
    ///
    /// The statements that tied with the winner are in [`Source::alternatives`]. A statement whose
    /// message is not a string literal has an empty template and never matches a line.
    ///
    /// Every call looks at every statement of the catalog.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::catalog;
    /// use komorebi_recorder::analysis::flavor::Flavor;
    /// use komorebi_recorder::analysis::logparse::Level;
    ///
    /// let catalog = catalog::of(Flavor::Macos);
    /// # if catalog.is_empty() { return; } // built without KOMOREBI_MAC_SRC: nothing is indexed
    ///
    /// let found = catalog
    ///     .find("komorebi::reaper", Level::Info, "reaping window: 4711")
    ///     .expect("the reaper logs this");
    /// assert_eq!(found.statement.file, "komorebi/src/reaper.rs");
    /// assert!(found.statement.template.starts_with("reaping window"));
    ///
    /// // no statement could have written this
    /// assert!(
    ///     catalog
    ///         .find("komorebi::process_event", Level::Info, "no such message at all")
    ///         .is_none()
    /// );
    /// ```
    ///
    /// The scoring is covered by the unit tests `a_message_matches_the_template_it_expands`,
    /// `a_line_that_could_come_from_two_statements_says_so`,
    /// `a_statement_that_is_all_placeholders_is_generic` and
    /// `nothing_is_found_for_a_message_no_statement_could_write`, and against real komorebi source
    /// by `the_handler_header_is_found_at_its_line` and `tests/code.rs`.
    #[must_use]
    pub fn find(&self, target: &str, level: Level, message: &str) -> Option<Source> {
        let level_name = match level {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        };

        let mut matches: Vec<(usize, &RawEntry)> = self
            .entries
            .iter()
            .filter(|entry| entry.target == target && entry.level == level_name)
            .filter_map(|entry| score(entry.template, message).map(|score| (score, entry)))
            .collect();
        matches.sort_by_key(|a| std::cmp::Reverse(a.0));

        let (best_score, best) = *matches.first()?;
        let tied: Vec<&(usize, &RawEntry)> = matches
            .iter()
            .skip(1)
            .filter(|(score, _)| *score == best_score)
            .collect();

        let confidence = if best_score <= 3 {
            // statements made of placeholders match any message; ties among them are expected
            Confidence::Generic
        } else if !tied.is_empty() {
            Confidence::Ambiguous
        } else if literals(best.template).len() == 1 {
            Confidence::Exact
        } else {
            Confidence::Likely
        };

        Some(Source {
            statement: Statement::of(best),
            confidence,
            alternatives: tied.iter().map(|(_, entry)| Statement::of(entry)).collect(),
        })
    }
}

impl Catalog {
    /// Returns where the span called `name` is declared: `process_event` is the `#[instrument]` on
    /// `WindowManager::process_event`. There can be more than one place, and none if no
    /// `#[instrument]` has that name.
    ///
    /// Each is a [`Statement`] whose `level` and `target` are empty and whose `template` is
    /// `#[instrument] <name>`.
    #[must_use]
    pub fn span(&self, name: &str) -> Vec<Statement> {
        self.spans
            .iter()
            .filter(|span| span.name == name)
            .map(|span| Statement {
                level: String::new(),
                target: String::new(),
                template: format!("#[instrument] {}", span.name),
                file: span.file.to_string(),
                line: span.line,
                function: span.function.to_string(),
                owner: span.owner.to_string(),
            })
            .collect()
    }

    /// Returns every statement of the module `target` (such as `komorebi::reaper`), in source
    /// order. Submodules are not included.
    #[must_use]
    pub fn statements_in(&self, target: &str) -> Vec<Statement> {
        self.entries
            .iter()
            .filter(|entry| entry.target == target)
            .map(Statement::of)
            .collect()
    }

    /// Returns the statements whose template contains `needle`, in source order. The template is
    /// compared in lowercase, so `needle` must already be: a needle with capitals matches nothing.
    #[must_use]
    pub fn statements_matching(&self, needle: &str) -> Vec<Statement> {
        self.entries
            .iter()
            .filter(|entry| entry.template.to_lowercase().contains(needle))
            .map(Statement::of)
            .collect()
    }
}

/// A part of komorebi and what it does, for someone reading its log.
///
/// Each flavor describes its parts ([`Rules::role`](super::flavor::Rules::role)): the same
/// module name can have another role in komorebi for Mac and komorebi for Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Role {
    /// A sentence saying what the part does.
    pub summary: &'static str,
    /// It changes komorebi's state without sending a subscriber notification, so a change in state
    /// with no event before it may be its doing.
    pub changes_state_silently: bool,
}

/// Returns the role of the module `target` among `roles`, a table of `(module, summary,
/// changes_state_silently)`.
///
/// The role whose module is the longest match wins: a module matches `target` if it is `target`
/// or `target` starts with the module and `::`. A target no module matches has the role "komorebi
/// internals", which changes no state silently.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::catalog::role_in;
///
/// let roles = [
///     ("komorebi", "komorebi itself", false),
///     ("komorebi::reaper", "removes windows that no longer exist", true),
/// ];
///
/// // the longest module wins, and submodules belong to their parent
/// assert!(role_in(&roles, "komorebi::reaper").changes_state_silently);
/// assert!(role_in(&roles, "komorebi::reaper::inner").changes_state_silently);
/// assert_eq!(role_in(&roles, "komorebi::window").summary, "komorebi itself");
///
/// // a module is matched whole: `komorebi::reaperx` is not a submodule of `komorebi::reaper`
/// assert!(!role_in(&roles, "komorebi::reaperx").changes_state_silently);
///
/// // and anything else is komorebi internals
/// assert_eq!(role_in(&roles, "tokio::net").summary, "komorebi internals");
/// ```
#[must_use]
pub fn role_in(roles: &[(&'static str, &'static str, bool)], target: &str) -> Role {
    roles
        .iter()
        .filter(|(module, _, _)| target == *module || target.starts_with(&format!("{module}::")))
        .max_by_key(|(module, _, _)| module.len())
        .map_or(
            Role {
                summary: "komorebi internals",
                changes_state_silently: false,
            },
            |(_, summary, silent)| Role {
                summary,
                changes_state_silently: *silent,
            },
        )
}

/// How the source this recorder was built against compares with the komorebi that was recorded.
///
/// A log line is mapped to a statement of the indexed source ([`Catalog::find`]), and that is only
/// the statement that wrote it if the indexed source is the source that ran. Skew is compared by
/// what differs in komorebi's source between the two commits, not by whether the commits are equal:
/// commits of this repository that touch nothing of komorebi's source change nothing about the
/// mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Skew {
    /// The commit the recorded komorebi was built from: the first commit among the recorded
    /// komorebi processes that reported one. Komorebi may report it abbreviated.
    pub recorded_commit: Option<String>,
    /// The commit whose source was indexed ([`Catalog::commit`]).
    pub indexed_commit: Option<String>,
    /// Whether the source tree had uncommitted changes when indexed ([`Catalog::dirty`]).
    pub indexed_dirty: bool,
    /// `same` (the source that ran and the source indexed are identical), `different` or
    /// `unknown` (it cannot be told, so nothing is vouched for).
    pub verdict: String,
    /// The files of komorebi's source that differ between the recorded commit and the indexed
    /// one, as `Statement::file` names them. A log line mapped into any other file is as good as
    /// if the commits were the same. Empty unless `verdict` is `different`, and also empty when it
    /// is `different` only because the indexed tree had uncommitted changes: which files they are
    /// is not recorded.
    pub changed_files: Vec<String>,
    /// What to say to the reader, `None` when the two agree.
    pub warning: Option<String>,
}

impl Skew {
    /// Returns `true` if and only if a statement in `file` may be different from the one that ran:
    /// the file is in [`Skew::changed_files`], or the verdict is `unknown`.
    ///
    /// A `different` verdict that rests only on uncommitted changes in the indexed tree has no
    /// changed files, so this is `false` for every file; the [`Skew::warning`] still says so.
    #[must_use]
    pub fn file_changed(&self, file: &str) -> bool {
        self.verdict == "unknown" || self.changed_files.iter().any(|changed| changed == file)
    }
}

fn git(root: &std::path::Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The files of komorebi's source that differ between two commits, or `None` if either commit is
/// not in the repository.
fn changed_files(
    root: &std::path::Path,
    from: &str,
    to: &str,
    source_paths: &[&str],
) -> Option<Vec<String>> {
    for commit in [from, to] {
        git(root, &["cat-file", "-e", &format!("{commit}^{{commit}}")])?;
    }

    let mut args = vec!["diff", "--name-only", from, to, "--"];
    args.extend(source_paths);
    let names = git(root, &args)?;
    Some(names.lines().map(str::to_string).collect())
}

/// Compares the komorebi that ran (`versions`, as recorded) with the source that was indexed, using
/// the repository at `root` to say which files differ.
///
/// `indexed` is the commit of the indexed source, `dirty` whether that tree had uncommitted
/// changes, and `source_paths` the directories of the checkout that were indexed: only differences
/// inside them count. The verdict is:
///
/// | verdict | when |
/// |---|---|
/// | `unknown` | the recorded commit or the indexed one is not known; or the commits differ and `root` is `None`, `git` cannot be run, or the repository does not have both commits |
/// | `same` | the commits are equal and the indexed tree was not dirty, or they differ but `git diff --name-only` lists nothing in `source_paths` and the tree was not dirty |
/// | `different` | anything else: files differ, or the indexed tree was dirty |
///
/// Commits are equal if one is a prefix of the other, since komorebi may report an abbreviated
/// commit. `root` is only asked, by running `git`, when the commits are not equal.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::catalog::skew_between;
/// use komorebi_recorder::sources::komorebi_version::KomorebiVersion;
///
/// let ran = KomorebiVersion {
///     pid: Some(4711),
///     version: "0.1.0".to_string(),
///     branch: None,
///     commit: Some("abc1234".to_string()),
/// };
/// let paths = ["komorebi/src"];
///
/// // the indexed source is at the commit that ran (which was reported abbreviated)
/// let same = skew_between(&[ran.clone()], Some("abc1234def5678"), false, None, &paths);
/// assert_eq!(same.verdict, "same");
/// assert!(same.warning.is_none());
/// assert!(!same.file_changed("komorebi/src/window.rs"));
///
/// // another commit, and no checkout to ask git which files differ: nothing is vouched for
/// let other = skew_between(&[ran], Some("fff0000"), false, None, &paths);
/// assert_eq!(other.verdict, "unknown");
/// assert!(other.warning.is_some());
/// assert!(other.file_changed("komorebi/src/window.rs"));
/// ```
///
/// Asking git is covered by the unit tests `a_commit_the_repository_does_not_have_is_unknown` and
/// `an_older_commit_differs_in_exactly_the_files_git_says`, which need a checkout of komorebi.
#[must_use]
pub fn skew_between(
    versions: &[KomorebiVersion],
    indexed: Option<&str>,
    dirty: bool,
    root: Option<&std::path::Path>,
    source_paths: &[&str],
) -> Skew {
    let recorded = versions.iter().find_map(|version| version.commit.clone());
    let short = |commit: &str| commit[..commit.len().min(10)].to_string();

    let mut skew = Skew {
        recorded_commit: recorded.clone(),
        indexed_commit: indexed.map(str::to_string),
        indexed_dirty: dirty,
        verdict: "unknown".to_string(),
        changed_files: Vec::new(),
        warning: None,
    };

    let (Some(recorded), Some(indexed)) = (recorded, indexed) else {
        skew.warning = Some(
            "which komorebi was recorded (or which source was indexed) is unknown, so source \
             locations may not match what ran"
                .to_string(),
        );
        return skew;
    };

    let changed = if recorded.starts_with(indexed) || indexed.starts_with(&recorded) {
        Some(Vec::new())
    } else {
        root.and_then(|root| changed_files(root, &recorded, indexed, source_paths))
    };

    match changed {
        None => {
            skew.warning = Some(format!(
                "the recorded komorebi was built from {} but its source is indexed at {}, and the \
                 repository cannot say how they differ (is the recorded commit fetched?): source \
                 locations may not match what ran",
                short(&recorded),
                short(indexed)
            ));
        }
        Some(files) if files.is_empty() && !dirty => skew.verdict = "same".to_string(),
        Some(files) => {
            skew.verdict = "different".to_string();
            skew.warning = Some(format!(
                "the recorded komorebi was built from {} and its source is indexed at {}{}: {}. \
                 Log lines mapped into those files may not be what ran; lines in other files are \
                 unaffected. Check out the recorded commit and rebuild the recorder to be sure",
                short(&recorded),
                short(indexed),
                if dirty {
                    " with uncommitted changes"
                } else {
                    ""
                },
                if files.is_empty() {
                    "the working tree has uncommitted changes to it".to_string()
                } else {
                    format!(
                        "{} file(s) differ ({})",
                        files.len(),
                        files
                            .iter()
                            .map(|file| file.rsplit('/').next().unwrap_or(file))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            ));
            skew.changed_files = files;
        }
    }

    skew
}

/// Returns [`skew_between`] for the source this build indexed for `flavor`, using the checkout at
/// `root` (see [`crate::annotations::default_code_root`]) to say which files differ.
///
/// If the flavor's catalog is empty the warning is replaced by one saying that nothing is indexed
/// and which variable to build with (`KOMOREBI_MAC_SRC` or `KOMOREBI_WINDOWS_SRC`); the verdict is
/// left as [`skew_between`] decided it. [`Analysis::skew`](super::Analysis::skew) calls this with
/// the session's flavor and recorded versions.
///
/// Covered by the unit test `an_empty_catalog_says_how_to_fill_it`.
#[must_use]
pub fn skew(flavor: Flavor, versions: &[KomorebiVersion], root: Option<&std::path::Path>) -> Skew {
    let catalog = of(flavor);
    let mut skew = skew_between(
        versions,
        catalog.commit(),
        catalog.dirty,
        root,
        catalog.source_paths,
    );

    if catalog.is_empty() {
        skew.warning = Some(format!(
            "this build of komorebi-recorder has no source indexed for komorebi for {flavor}, so \
             log lines cannot be mapped to code; build it with {} pointing at a checkout of \
             komorebi for {flavor}",
            match flavor {
                Flavor::Macos => "KOMOREBI_MAC_SRC",
                Flavor::Windows => "KOMOREBI_WINDOWS_SRC",
            }
        ));
    }

    skew
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn mac() -> &'static Catalog {
        of(Flavor::Macos)
    }

    /// The directories of the Mac checkout that its catalog indexes.
    const MAC_PATHS: &[&str] = &["komorebi/src", "komorebi-client/src"];

    fn commit() -> Option<&'static str> {
        mac().commit()
    }

    fn find(target: &str, level: Level, message: &str) -> Option<Source> {
        mac().find(target, level, message)
    }

    #[test]
    fn templates_split_around_their_placeholders() {
        assert_eq!(literals("plain"), ["plain"]);
        assert_eq!(literals("a {} b {x:?} c"), ["a ", " b ", " c"]);
        assert_eq!(literals("{}"), ["", ""]);
        assert_eq!(literals("{{braces}} {x}"), ["{braces} ", ""]);
    }

    #[test]
    fn a_message_matches_the_template_it_expands() {
        assert_eq!(
            score("reaping window: {window_id}", "reaping window: 4711"),
            Some(16)
        );
        assert!(score("reaping window: {window_id}", "removing window: 4711").is_none());
        assert!(score("a {} b {} c", "a 1 b 2 c").is_some());
        assert!(score("a {} b {} c", "a 1 c b 2").is_none());
        // exact templates must match whole
        assert!(score("focusing window", "focusing window").is_some());
        assert!(score("focusing window", "focusing window now").is_none());
        // the start and the end both have to be there
        assert!(score("x {} y", "x 1 z").is_none());
        assert!(score("x {} y", "w 1 y").is_none());
    }

    #[test]
    fn the_catalog_was_built_from_komorebis_source() {
        assert!(
            mac().files_scanned() > 20,
            "only {} files were indexed",
            mac().files_scanned()
        );
        assert!(
            mac().len() > 200,
            "only {} log statements were indexed",
            mac().len()
        );
        assert!(mac().spans.len() > 5);
    }

    #[test]
    fn the_handler_header_is_found_at_its_line() {
        let found = find(
            "komorebi::process_event",
            Level::Info,
            "processing event: MoveEnd for process 20934 with notification AXWindowMoved",
        )
        .expect("the header statement");

        assert_eq!(found.statement.file, "komorebi/src/process_event.rs");
        assert_eq!(found.statement.function, "process_event");
        assert_eq!(found.statement.owner, "WindowManager");
        assert_eq!(found.confidence, Confidence::Likely);
        assert!(found.statement.line > 100);

        // and the line it names really is that statement
        let source =
            std::fs::read_to_string(crate::testkit::komorebi_mac_src().join(&found.statement.file))
                .expect("source");
        let text = source
            .lines()
            .nth(found.statement.line - 1)
            .expect("the line exists");
        assert!(text.contains("tracing::info!"), "{text}");
    }

    #[test]
    fn a_line_that_could_come_from_two_statements_says_so() {
        // two statements of the same level in one module with the same text
        let mut seen: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
        for entry in mac()
            .entries
            .iter()
            .filter(|entry| !entry.template.contains('{') && entry.template.len() > 3)
        {
            *seen
                .entry((entry.target, entry.level, entry.template))
                .or_default() += 1;
        }

        let duplicate = seen
            .into_iter()
            .find(|(_, count)| *count > 1)
            .map(|((target, level, template), _)| (target, level, template));
        if let Some((target, level, template)) = duplicate {
            let level = match level {
                "ERROR" => Level::Error,
                "WARN" => Level::Warn,
                "INFO" => Level::Info,
                "DEBUG" => Level::Debug,
                _ => Level::Trace,
            };
            let found = find(target, level, template).expect("found");
            assert_eq!(found.confidence, Confidence::Ambiguous);
            assert!(!found.alternatives.is_empty());
        }
    }

    #[test]
    fn a_statement_that_is_all_placeholders_is_generic() {
        // `tracing::error!("{}", error)` in the process_event listener
        let found = find(
            "komorebi::process_event",
            Level::Error,
            "something went wrong",
        )
        .expect("a generic statement");
        assert_eq!(found.confidence, Confidence::Generic);
    }

    #[test]
    fn nothing_is_found_for_a_message_no_statement_could_write() {
        assert!(
            find(
                "komorebi::process_event",
                Level::Info,
                "no such message at all"
            )
            .is_none()
        );
        assert!(find("komorebi::nowhere", Level::Info, "processing event: x").is_none());
    }

    #[test]
    fn every_statement_of_the_real_log_sample_is_found_in_the_source() {
        let sample = include_str!("../../tests/fixtures/komorebi_plaintext_sample.log");
        let lines = crate::analysis::logparse::parse(sample.as_bytes());
        let structured: Vec<_> = lines.iter().filter(|line| !line.continuation).collect();

        let missing: Vec<String> = structured
            .iter()
            .filter(|line| {
                let (Some(target), Some(level)) = (&line.target, line.level) else {
                    return false;
                };
                find(target, level, &line.message).is_none()
            })
            .map(|line| format!("{}: {}", line.number, line.message))
            .collect();

        // the sample is from the installed binary, which can be a different commit than the
        // source: a few lines may have moved or changed, but the catalog has to be broadly right
        assert!(
            missing.len() * 10 <= structured.len(),
            "{} of {} lines were not found: {missing:#?}",
            missing.len(),
            structured.len()
        );
    }

    fn version(commit: &str) -> KomorebiVersion {
        KomorebiVersion {
            pid: None,
            version: "0.1.0".to_string(),
            branch: None,
            commit: Some(commit.to_string()),
        }
    }

    fn repo() -> std::path::PathBuf {
        crate::testkit::komorebi_mac_src()
    }

    #[test]
    fn an_unknown_komorebi_or_source_is_unknown_not_assumed_the_same() {
        assert_eq!(
            skew_between(&[], Some("abc1234"), false, None, MAC_PATHS).verdict,
            "unknown"
        );
        assert!(
            skew_between(&[], Some("abc1234"), false, None, MAC_PATHS)
                .warning
                .is_some()
        );
        assert_eq!(
            skew_between(&[version("abc1234")], None, false, None, MAC_PATHS).verdict,
            "unknown"
        );
    }

    #[test]
    fn the_same_commit_is_the_same_source() {
        let skew = skew_between(
            &[version("abc1234def")],
            Some("abc1234def"),
            false,
            None,
            MAC_PATHS,
        );
        assert_eq!(skew.verdict, "same");
        assert!(skew.warning.is_none());
        assert!(!skew.file_changed("komorebi/src/window.rs"));

        // but not if the source had uncommitted changes when indexed
        let dirty = skew_between(
            &[version("abc1234def")],
            Some("abc1234def"),
            true,
            Some(&repo()),
            MAC_PATHS,
        );
        assert_eq!(dirty.verdict, "different");
    }

    #[test]
    fn a_commit_the_repository_does_not_have_is_unknown() {
        let skew = skew_between(
            &[version("0000000000000000000000000000000000000000")],
            commit(),
            false,
            Some(&repo()),
            MAC_PATHS,
        );
        assert_eq!(skew.verdict, "unknown");
        assert!(
            skew.warning
                .as_deref()
                .expect("warning")
                .contains("cannot say how they differ")
        );
        // when it cannot be told, nothing is vouched for
        assert!(skew.file_changed("komorebi/src/window.rs"));
    }

    #[test]
    fn an_older_commit_differs_in_exactly_the_files_git_says() {
        let Some(indexed) = commit() else { return };
        // the commit before the latest change to komorebi's source
        let Some(history) = git(&repo(), &["log", "-2", "--format=%H", "--", "komorebi/src"])
        else {
            return;
        };
        let Some(older) = history.lines().nth(1) else {
            return;
        };

        let skew = skew_between(
            &[version(older)],
            Some(indexed),
            false,
            Some(&repo()),
            MAC_PATHS,
        );
        let expected = git(
            &repo(),
            &[
                "diff",
                "--name-only",
                older,
                indexed,
                "--",
                "komorebi/src",
                "komorebi-client/src",
            ],
        )
        .expect("git diff");
        assert_eq!(skew.changed_files.join("\n"), expected);
        assert_eq!(
            skew.verdict,
            if expected.is_empty() {
                "same"
            } else {
                "different"
            }
        );
        for file in expected.lines() {
            assert!(skew.file_changed(file));
        }
        assert!(!skew.file_changed("komorebi/src/a_file_that_never_changed.rs"));
    }

    fn rust_files(directory: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(directory).expect("read_dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    /// If komorebi gains a way of logging the index cannot see (a macro that wraps `tracing`, a
    /// log statement inside another macro), the count of statements in the source stops matching
    /// what was indexed and this fails, rather than `code` silently missing lines.
    #[test]
    fn every_log_statement_in_komorebis_source_is_indexed() {
        let root = crate::testkit::komorebi_mac_src().join("komorebi/src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);

        let mut in_source = 0_usize;
        for file in files {
            for line in std::fs::read_to_string(&file).expect("read").lines() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                for level in ["error", "warn", "info", "debug", "trace"] {
                    in_source += code.matches(&format!("tracing::{level}!")).count();
                }
            }
        }

        let indexed = mac()
            .entries
            .iter()
            .filter(|entry| entry.file.starts_with("komorebi/src/"))
            .count();
        assert_eq!(
            indexed, in_source,
            "{in_source} tracing statements in komorebi's source but {indexed} were indexed"
        );
    }

    #[test]
    fn each_flavor_has_a_catalog_of_its_own_source() {
        // the Windows catalog is built only when KOMOREBI_WINDOWS_SRC names a checkout; where it
        // is, its statements are komorebi for Windows's, not the Mac's
        let windows = of(Flavor::Windows);
        if windows.is_empty() {
            assert_eq!(windows.files_scanned(), 0);
            assert_eq!(windows.commit(), None);
            return;
        }

        assert!(windows.files_scanned() > 20);
        assert!(
            windows
                .entries
                .iter()
                .any(|entry| entry.target == "komorebi::reaper"),
            "komorebi for Windows has a reaper"
        );
        assert!(
            windows
                .entries
                .iter()
                .all(|entry| !entry.file.contains('\\')),
            "catalog paths are always `/`-separated"
        );
        assert!(
            !windows
                .entries
                .iter()
                .any(|entry| entry.target == "komorebi::macos_api"),
            "no Mac module in the Windows catalog"
        );
    }

    #[test]
    fn an_empty_catalog_says_how_to_fill_it() {
        let skew = skew(Flavor::Windows, &[], None);
        if of(Flavor::Windows).is_empty() {
            let warning = skew.warning.expect("a warning");
            assert!(warning.contains("KOMOREBI_WINDOWS_SRC"), "{warning}");
        }
    }
}
