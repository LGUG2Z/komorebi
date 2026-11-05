//! The flavors of komorebi (for Mac and for Windows), and what the output of each one means.
//!
//! The host a session was *recorded on* (see [`crate::platform`]) and the *komorebi* it was
//! recording are separate things: a session recorded on Windows is investigated on a Mac, and the
//! other way round. Everything that reads komorebi's output (its events, its `State`, its log
//! and its source) is written in komorebi's vocabulary, and that differs between the two:
//!
//! - what an event looks like on the wire and what its fields mean (a process id and an
//!   accessibility notification on macOS, a window handle and a `WinEvent` on Windows);
//! - which events move or resize a window, and which are komorebi making things up;
//! - how komorebi hides a window, and so whether a window's position shows that it is hidden;
//! - how its log marks where a handler starts and ends;
//! - which of its threads and modules do what.
//!
//! The module defines a pair of roles:
//!
//! - **A [`Flavor`] is a name for a komorebi.** It is a two-valued enum, recorded in the session's
//!   manifest, that identifies which komorebi wrote the files being read.
//! - **A type that implements [`Rules`] is the vocabulary of that komorebi.** It answers the
//!   questions the analysis has about komorebi's events, `State`, log and source. [`MacRules`]
//!   (in [`mac`]) and [`WindowsRules`] (in [`windows`]) are the two implementors, and
//!   [`Flavor::rules`] is the one place that turns a name into rules.
//!
//! The analysis is written once, against `&dyn Rules`: the functions that read an event, classify
//! it, group log lines into handlers and parse a `State` take the rules as an argument or reach
//! them through the `Snapshot` they read. The host does not take part: [`Flavor::of_host`] is only
//! the flavor a recorder writes into the manifest of a session it records.
//!
//! # The questions, and who answers them
//!
//! Each method of [`Rules`] is one question. Every row of this table is a place where the two
//! flavors differ; the third column names the code that asks.
//!
//! | Method | Answers | Asked by | komorebi for Mac | komorebi for Windows |
//! |---|---|---|---|---|
//! | [`describe_event`](Rules::describe_event) | what a notification's JSON means | `EventRecord::from_row` | `[{"type": "Accessibility", ...}, pid, window]` | `["<WinEvent>", window]` |
//! | [`is_geometry_event`](Rules::is_geometry_event) | which events start or end a move or resize | [`classify`] | `MoveStart`, `MoveEnd`, `ResizeStart`, `ResizeEnd` | `MoveResizeStart`, `MoveResizeEnd` |
//! | [`classify_window_manager`](Rules::classify_window_manager) | what an OS-reported event was, and why | [`classify`] | a button release komorebi made up is [`Class::Manual`] | `Cloak` and `Uncloak` are [`Class::Lifecycle`] |
//! | [`classify_monitor`](Rules::classify_monitor) | what a display or machine notification was | [`classify`] | the default: [`Class::Monitor`] | lock, suspend and virtual desktop notifications are [`Class::Lock`] |
//! | [`window_id`](Rules::window_id) | the id a window has in `State` | `Snapshot::parse` | `id` | `hwnd` |
//! | [`window_is_unreadable`](Rules::window_is_unreadable) | whether komorebi failed to read a window | `Snapshot::parse` | the default: never | the title starts with `could not get window` |
//! | [`hides_windows_by_position`](Rules::hides_windows_by_position) | whether a window's position shows that it is hidden | `Snapshot::hides_windows_by_position` | yes: parked at a corner | no: cloaked where it is |
//! | [`is_hidden_position`](Rules::is_hidden_position) | whether a rect is where a hidden window is parked | `Snapshot::is_hidden_position` | a one-point sliver inside the display | never |
//! | [`block_header`](Rules::block_header) | whether a log line starts a handler's block | `blocks::build` | the `processing event:` and `processing command:` lines | the first line of a `process_event{...}` or `process_command{...}` span whose fields differ from the open block's |
//! | [`is_block_end`](Rules::is_block_end) | whether a log line ends a handler's block | `blocks::build` | `processed command: ...`, for commands only | `processed: ...` for events, `processed` for commands |
//! | [`role`](Rules::role) | what the part of komorebi that logged a line does | the `code` and `trace` answers | `mac::ROLES` | `windows::ROLES` |
//! | [`slow_handler_cause`](Rules::slow_handler_cause) | what a slow handler was probably doing | the `overview` and `trace` answers | accessibility calls | Win32 calls |
//! | [`verified`](Rules::verified) | whether the rules were checked against a recording | `Analysis::warnings` | the default: yes | no |
//! | [`flavor`](Rules::flavor) | which flavor the rules are for | tests | [`Flavor::Macos`] | [`Flavor::Windows`] |
//!
//! # What is guaranteed, and by what
//!
//! | Rule | Enforced by | Proof | Escape hatch |
//! |---|---|---|---|
//! | An implementor of [`Rules`] answers every question that has no default | type system: the methods have no default | `compile_fail,E0046` on [`Rules`] | none |
//! | An implementor can be shared between threads | type system: `Sync` is a supertrait of [`Rules`] | the trait's definition | none |
//! | [`Flavor::rules`] has rules for every flavor | type system: an exhaustive `match` | a new [`Flavor`] variant without an arm fails to compile in this module | none |
//! | `flavor.rules().flavor()` is `flavor` | test | `each_flavor_has_rules_of_its_own_and_only_the_checked_ones_say_so` | none |
//! | A manifest with no flavor is read as komorebi for Mac | runtime: a serde default | `a_session_with_no_flavor_recorded_is_komorebi_for_mac` | none |
//! | Rules that were not checked against a recording of komorebi say so | framework path: `Analysis::warnings` adds a warning when [`verified`](Rules::verified) is `false`. The method defaults to `true`, so an implementor has to override it | `the_analysis_says_the_windows_rules_are_not_yet_checked_against_a_recording` and `a_mac_session_does_not_get_that_warning` in `tests/windows_flavor.rs` | none |
//! | A flavor for which `hides_windows_by_position` is `false` answers `false` to `is_hidden_position` | convention only | [`WindowsRules`] follows it | none |
//! | A notification that cannot be read is [`Family::Unknown`], not a panic | test, for each implementor: `nonsense_is_unknown_not_a_panic` in [`mac`] and in [`windows`]. For a new implementor, convention only | those tests | none |
//! | Code that reads komorebi's output asks [`Rules`] and does not match on [`Flavor`] | convention only | none | the two cases below |
//!
//! Two things that depend on the flavor are keyed by [`Flavor`] and not asked of [`Rules`]: the
//! index of komorebi's source that log lines are mapped to (`catalog::of`), and the places the
//! log and a checkout of the source are looked for on disk (`komorebi_log::log_dir_candidates`
//! and `annotations::default_code_root`).
//!
//! # One session, end to end
//!
//! 1. `Analysis::open` reads the manifest. Its `komorebi.flavor` is what `Analysis::flavor`
//!    returns; a manifest that has none gives [`Flavor::Macos`].
//! 1. `Analysis::events` calls `events::load` with `flavor.rules()`, which turns each line of
//!    `events.jsonl` into an [`EventRecord`] with [`describe_event`](Rules::describe_event).
//! 1. [`classify`] labels each record. It asks
//!    [`classify_window_manager`](Rules::classify_window_manager) or
//!    [`classify_monitor`](Rules::classify_monitor), by the record's [`Family`], and
//!    [`is_geometry_event`](Rules::is_geometry_event) when it decides whether a button release
//!    that komorebi made up ended a real drag.
//! 1. `blocks::build` walks the log: `block_header` for every line, and
//!    `is_block_end` for every line inside an open block.
//! 1. `Snapshot::parse` reads each `State`, asking [`window_id`](Rules::window_id) and
//!    [`window_is_unreadable`](Rules::window_is_unreadable) for every window. The snapshot keeps
//!    its flavor for [`hides_windows_by_position`](Rules::hides_windows_by_position) and
//!    `is_hidden_position`.
//! 1. Answers that explain a log line or a slow handler ask `role` and
//!    [`slow_handler_cause`](Rules::slow_handler_cause). `Analysis::warnings` asks
//!    [`verified`](Rules::verified).
//!
//! [`MacRules`]: mac::MacRules
//! [`WindowsRules`]: windows::WindowsRules
//! [`classify`]: super::classify::classify
//! [`Family`]: super::events::Family
//! [`Family::Unknown`]: super::events::Family::Unknown

pub mod kde;
pub mod mac;
pub mod windows;

use super::blocks::Block;
use super::blocks::BlockKind;
use super::catalog::Role;
use super::classify::Class;
use super::events::EventInfo;
use super::events::EventRecord;
use super::logparse::Line;
use super::state::Monitor;
use super::state::Rect;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// A name for a komorebi: komorebi for Mac or komorebi for Windows.
///
/// A session records which of the two it was watching, in its manifest, because what komorebi's
/// events, `State` and log say differs between them. The flavor is the one place the analysis
/// learns which komorebi it is reading: [`Flavor::rules`] returns the [`Rules`] that know that
/// komorebi's vocabulary.
///
/// A flavor names a komorebi, not an operating system. A recorder watches the komorebi of the host
/// it runs on ([`Flavor::of_host`]) and writes that flavor into the manifest, and whoever investigates
/// the session later uses the flavor from the manifest, whatever machine they are on: a session
/// recorded on Windows is read on a Mac with the Windows rules.
///
/// The default is [`Flavor::Macos`], which is also what a manifest that has no flavor means.
///
/// # Serialization
///
/// A flavor is spelled in lowercase (`"macos"`, `"windows"`) wherever it is serialized, and the
/// type has a JSON Schema. [`Flavor::label`] and the [`Display`](std::fmt::Display)
/// implementation spell it for prose (`macOS`, `Windows`).
///
/// # Example
///
/// Each flavor's rules read the wire format of its komorebi. The same call, on the same
/// type, gives a process id and an accessibility notification for one, and a window handle, a
/// program and a `WinEvent` for the other:
///
/// ```rust
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use serde_json::json;
///
/// let on_mac = json!({
///     "type": "MoveEnd",
///     "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, 20934, 4711]
/// });
/// let info = Flavor::Macos.rules().describe_event(&on_mac);
/// assert_eq!(info.pid, Some(20934));
/// assert_eq!(info.summary(), "MoveEnd pid 20934 window 4711 AXWindowMoved");
///
/// let on_windows = json!({
///     "type": "FocusChange",
///     "content": ["SystemForeground", {"hwnd": 131072, "title": "a title", "exe": "chrome.exe"}]
/// });
/// let info = Flavor::Windows.rules().describe_event(&on_windows);
/// assert_eq!(info.pid, None);
/// assert_eq!(info.exe.as_deref(), Some("chrome.exe"));
/// assert_eq!(info.summary(), "FocusChange chrome.exe window 131072 SystemForeground");
///
/// // a manifest spells a flavor in lowercase, and has the Mac's when it says nothing
/// assert_eq!(serde_json::to_string(&Flavor::Windows)?, r#""windows""#);
/// assert_eq!(serde_json::from_str::<Flavor>(r#""macos""#)?, Flavor::Macos);
/// assert_eq!(Flavor::default(), Flavor::Macos);
///
/// // and prose says it the way people do
/// assert_eq!(Flavor::Macos.label(), "macOS");
/// assert_eq!(Flavor::Windows.to_string(), "Windows");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Flavor {
    /// komorebi for Mac. It is also what a manifest that has no flavor means, because every session
    /// recorded before manifests said which komorebi they watched was recorded on a Mac.
    #[default]
    Macos,
    /// komorebi for Windows.
    Windows,
    /// komorebi for KDE Plasma.
    Kde,
}

impl Flavor {
    /// Returns the flavor of the komorebi that runs on this machine: there is one per OS, and a
    /// recorder watches the one on its own host.
    ///
    /// This is [`Flavor::Windows`] when the crate is built for Windows, [`Flavor::Kde`] when it is built
    /// for Linux and [`Flavor::Macos`] for every other target, including the `unsupported` host. Use it to say which komorebi a
    /// recording made *here* is of; to read a session, use the flavor in its manifest.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::flavor::Flavor;
    ///
    /// assert_eq!(Flavor::of_host() == Flavor::Windows, cfg!(windows));
    /// ```
    #[must_use]
    pub fn of_host() -> Self {
        if cfg!(windows) {
            Flavor::Windows
        } else if cfg!(target_os = "linux") {
            Flavor::Kde
        } else {
            Flavor::Macos
        }
    }

    /// Returns the [`Rules`] of this flavor: the vocabulary of this komorebi.
    ///
    /// This is the only place a flavor is turned into rules. The rules are a shared, stateless
    /// value of [`MacRules`](mac::MacRules) or [`WindowsRules`](windows::WindowsRules), so calling
    /// this is free and the result may be kept for as long as the program runs.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::flavor::Flavor;
    ///
    /// for flavor in [Flavor::Macos, Flavor::Windows] {
    ///     assert_eq!(flavor.rules().flavor(), flavor);
    /// }
    ///
    /// // only the rules of a komorebi that was checked against a recording say so
    /// assert!(Flavor::Macos.rules().verified());
    /// assert!(!Flavor::Windows.rules().verified());
    /// ```
    #[must_use]
    pub fn rules(self) -> &'static dyn Rules {
        match self {
            Flavor::Macos => &mac::MacRules,
            Flavor::Windows => &windows::WindowsRules,
            Flavor::Kde => &kde::KdeRules,
        }
    }

    /// Returns the name to use in prose: `macOS` or `Windows`.
    ///
    /// This is also what the [`Display`](std::fmt::Display) implementation writes. The serialized
    /// form is different: `macos` or `windows`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Flavor::Macos => "macOS",
            Flavor::Windows => "Windows",
            Flavor::Kde => "KDE",
        }
    }
}

impl std::fmt::Display for Flavor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// The start of a handler's block of log lines.
///
/// [`Rules::block_header`] returns one for the first line of a block, describing the handler that
/// line begins; `blocks::build` makes the `Block` from it. A field is `None` when the flavor's log
/// does not give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockHeader {
    /// Whether the handler is handling an event or a command (a `BlockKind`).
    pub kind: BlockKind,
    /// The event or command name: `MoveEnd`, `FocusWorkspaceNumber`, ...
    pub name: String,
    /// The process the event is about. komorebi for Mac logs it, and komorebi for Windows does not.
    pub pid: Option<i32>,
    /// The window handle, where the log gives one instead of a process. komorebi for Windows logs
    /// it as the span field `hwnd`.
    pub window_id: Option<u32>,
    /// The notification the OS reported, e.g. `AXWindowMoved` on a Mac or `SystemForeground` on
    /// Windows; `None` for a command.
    pub notification: Option<String>,
}

/// The vocabulary of one flavor of komorebi, as the questions the analysis asks of it.
///
/// The role of this trait is to answer, for one komorebi, every question the analysis has about
/// komorebi's vocabulary: how an event is written, what a `State` calls a window, where a
/// hidden window is, what a handler's first and last log line look like, and what each of
/// komorebi's modules does. Each method is one question. The [module documentation](self) has the
/// table of them, with what each flavor answers, and the guarantee table.
///
/// - **The analysis calls it.** It is given a `&dyn Rules` (from [`Flavor::rules`]) and asks it,
///   and has no way to tell which flavor it was given.
/// - **A flavor implements it.** [`MacRules`](mac::MacRules) is komorebi for Mac and
///   [`WindowsRules`](windows::WindowsRules) is komorebi for Windows. A third implementor is
///   written only for a third komorebi. To be chosen by name it also needs a [`Flavor`] variant,
///   because [`Flavor::rules`] is an exhaustive `match`. The functions that take a `&dyn Rules`
///   (`events::load`, `EventRecord::from_row` and
///   [`classify`](super::classify::classify), and `blocks::build`) accept any implementor,
///   whether or not `Flavor::rules` can return it.
///
/// # The contract
///
/// - **Most methods have no default, and a few do.** An implementor must write
///   [`flavor`](Rules::flavor), [`window_id`](Rules::window_id),
///   [`hides_windows_by_position`](Rules::hides_windows_by_position),
///   [`describe_event`](Rules::describe_event), [`is_geometry_event`](Rules::is_geometry_event),
///   [`classify_window_manager`](Rules::classify_window_manager),
///   `is_hidden_position`, `block_header`,
///   `is_block_end`, `role` and
///   [`slow_handler_cause`](Rules::slow_handler_cause); a missing one is a compile error (see
///   below). [`verified`](Rules::verified), [`classify_monitor`](Rules::classify_monitor) and
///   [`window_is_unreadable`](Rules::window_is_unreadable) have a default that suits a flavor
///   with nothing to say about them. A default is not a guarantee: an implementor may override it.
/// - **Answers come from the arguments.** Every method takes `&self`, and the trait requires
///   `Sync`, so rules are shared between threads and carry no state. The one piece of context a
///   method may need is passed in: `block_header` is given the block that
///   is open.
/// - **Unreadable input is an answer, not a panic.** komorebi's output is read after the fact, from
///   a session that may be from a newer komorebi than this recorder. [`describe_event`] answers
///   [`Family::Unknown`] for JSON it does not understand, and [`window_id`] answers `None` for
///   JSON that is not a window. Both implementors have a test for `describe_event`; nothing
///   enforces the rule for a new implementor.
/// - **Hiding has two questions, and they must agree.** A flavor for which
///   [`hides_windows_by_position`](Rules::hides_windows_by_position) is `false` should answer `false` to every call of
///   `is_hidden_position`. Some checks ask only the second, and would otherwise report a
///   window as parked. Only convention enforces this; [`WindowsRules`](windows::WindowsRules)
///   follows it.
/// - **A block stops collecting lines in one of two ways.** `block_header` is asked of every
///   timestamped line of the log and starts a block when it returns `Some`. A block that is still
///   open stops collecting lines when the next header starts a new block, or when
///   `is_block_end` returns `true`. `is_block_end` is asked only of the lines that were added to
///   the open block, never of the line that started it. A flavor whose log has no end line answers
///   `false`, and its blocks stop at the next header.
///
/// [`describe_event`]: Rules::describe_event
/// [`window_id`]: Rules::window_id
/// [`hides_windows_by_position`]: Rules::hides_windows_by_position
/// [`Family::Unknown`]: super::events::Family::Unknown
///
/// # Example implementation
///
/// An implementor that reads komorebi for Mac, by delegating to [`MacRules`](mac::MacRules), but
/// that reports it was never checked against a recording. [`verified`](Rules::verified) is the one
/// method written beyond the ones that have no default, and the others keep theirs:
///
/// ```rust
/// # use komorebi_recorder::analysis::blocks::{Block, BlockKind};
/// # use komorebi_recorder::analysis::catalog::Role;
/// # use komorebi_recorder::analysis::classify::Class;
/// # use komorebi_recorder::analysis::events::{EventInfo, EventRecord};
/// # use komorebi_recorder::analysis::flavor::mac::MacRules;
/// # use komorebi_recorder::analysis::flavor::{BlockHeader, Flavor, Rules};
/// # use komorebi_recorder::analysis::logparse::Line;
/// # use komorebi_recorder::analysis::state::{Monitor, Rect};
/// # use serde_json::Value;
/// struct Unchecked;
///
/// impl Rules for Unchecked {
///     fn flavor(&self) -> Flavor { Flavor::Macos }
///     fn verified(&self) -> bool { false }
///     fn window_id(&self, window: &Value) -> Option<u64> { MacRules.window_id(window) }
///     fn hides_windows_by_position(&self) -> bool { MacRules.hides_windows_by_position() }
///     fn describe_event(&self, event: &Value) -> EventInfo { MacRules.describe_event(event) }
///     fn is_geometry_event(&self, name: &str) -> bool { MacRules.is_geometry_event(name) }
///     fn classify_window_manager(&self, event: &EventRecord) -> (Class, &'static str) {
///         MacRules.classify_window_manager(event)
///     }
///     fn is_hidden_position(&self, rect: &Rect, monitor: &Monitor) -> bool {
///         MacRules.is_hidden_position(rect, monitor)
///     }
///     fn block_header(&self, line: &Line, open: Option<&Block>) -> Option<BlockHeader> {
///         MacRules.block_header(line, open)
///     }
///     fn is_block_end(&self, line: &Line, kind: BlockKind) -> bool {
///         MacRules.is_block_end(line, kind)
///     }
///     fn role(&self, target: &str) -> Role { MacRules.role(target) }
///     fn slow_handler_cause(&self) -> &'static str { MacRules.slow_handler_cause() }
/// }
///
/// let rules: &dyn Rules = &Unchecked;
/// let event = serde_json::json!({"type": "FocusWorkspaceNumber", "content": 2});
/// assert_eq!(rules.describe_event(&event).argument.as_deref(), Some("2"));
/// assert!(!rules.verified());
/// // a method that was not written has its default
/// assert!(!rules.window_is_unreadable(&serde_json::json!({"id": 1})));
/// ```
///
/// # Example: a missing method is refused
///
/// An implementor that leaves out a method that has no default does not compile. This is the
/// implementor above without `slow_handler_cause`:
///
/// ```rust,compile_fail,E0046
/// # use komorebi_recorder::analysis::blocks::{Block, BlockKind};
/// # use komorebi_recorder::analysis::catalog::Role;
/// # use komorebi_recorder::analysis::classify::Class;
/// # use komorebi_recorder::analysis::events::{EventInfo, EventRecord};
/// # use komorebi_recorder::analysis::flavor::mac::MacRules;
/// # use komorebi_recorder::analysis::flavor::{BlockHeader, Flavor, Rules};
/// # use komorebi_recorder::analysis::logparse::Line;
/// # use komorebi_recorder::analysis::state::{Monitor, Rect};
/// # use serde_json::Value;
/// struct Unchecked;
///
/// impl Rules for Unchecked {
///     fn flavor(&self) -> Flavor { Flavor::Macos }
///     fn verified(&self) -> bool { false }
///     fn window_id(&self, window: &Value) -> Option<u64> { MacRules.window_id(window) }
///     fn hides_windows_by_position(&self) -> bool { MacRules.hides_windows_by_position() }
///     fn describe_event(&self, event: &Value) -> EventInfo { MacRules.describe_event(event) }
///     fn is_geometry_event(&self, name: &str) -> bool { MacRules.is_geometry_event(name) }
///     fn classify_window_manager(&self, event: &EventRecord) -> (Class, &'static str) {
///         MacRules.classify_window_manager(event)
///     }
///     fn is_hidden_position(&self, rect: &Rect, monitor: &Monitor) -> bool {
///         MacRules.is_hidden_position(rect, monitor)
///     }
///     fn block_header(&self, line: &Line, open: Option<&Block>) -> Option<BlockHeader> {
///         MacRules.block_header(line, open)
///     }
///     fn is_block_end(&self, line: &Line, kind: BlockKind) -> bool {
///         MacRules.is_block_end(line, kind)
///     }
///     fn role(&self, target: &str) -> Role { MacRules.role(target) }
///     // no `slow_handler_cause`
/// }
/// ```
///
/// The error is E0046, `not all trait items implemented, missing: slow_handler_cause`. The same
/// refusal applies to each method without a default, so a new flavor cannot leave
/// one of komorebi's questions unanswered.
pub trait Rules: Sync {
    /// Returns the flavor these rules are for.
    ///
    /// `flavor.rules().flavor()` is `flavor` for both flavors.
    fn flavor(&self) -> Flavor;

    /// Returns whether these rules have been checked against a real recording of the komorebi
    /// they describe, rather than only written from its source.
    ///
    /// The default is `true`. `Analysis::warnings` adds a
    /// warning to the answers about a session whose flavor has unverified rules, saying that this
    /// build reads komorebi for that flavor from its source and has not yet checked it against a
    /// recording. A flavor whose rules were not checked must override this to say so.
    ///
    /// komorebi for Mac keeps the default. komorebi for Windows answers `false`.
    fn verified(&self) -> bool {
        true
    }

    /// Returns what a display or machine notification ([`Family::Monitor`], not a window event or
    /// a command) was, as a [`Class`] and a sentence giving the reason. The sentence is what a reader
    /// sees as the event's reason.
    ///
    /// The default is `(Class::Monitor, "a display changed")`. komorebi for Mac keeps it. komorebi
    /// for Windows overrides it, because its monitor notifications also carry the machine's
    /// state: a session locked or unlocked, a sleep or a wake, and a virtual desktop left or
    /// returned to are [`Class::Lock`].
    ///
    /// [`Family::Monitor`]: super::events::Family::Monitor
    fn classify_monitor(&self, _event: &EventRecord) -> (Class, &'static str) {
        (Class::Monitor, "a display changed")
    }

    /// Returns the id a window is known by in `State`, or `None` if `window` is not a window.
    ///
    /// On a Mac that is the window's `id`, and on Windows it is the handle, `hwnd`; a handle that
    /// is negative is not an id. The value is the key under which a `Snapshot` lists the window,
    /// and what an event's window is looked up by.
    fn window_id(&self, window: &Value) -> Option<u64>;

    /// Returns whether `window` is one komorebi could not read when it wrote the state (it had
    /// been destroyed), so that its title, program and rect say nothing.
    ///
    /// A window that is unreadable is kept, with its id, but has no rect: a zero rect would read as
    /// a window parked at the corner of the screen.
    ///
    /// The default is `false`. komorebi for Mac keeps it. komorebi for Windows reads a window's
    /// title, program and rect when it serializes the state, and writes `could not get window
    /// title` (and likewise for the program and class) in place of one it could not read, so its
    /// rules look for that.
    fn window_is_unreadable(&self, _window: &Value) -> bool {
        false
    }

    /// Returns whether komorebi hides a window by moving it to a corner, so that whether a window
    /// is hidden can be read from its position.
    ///
    /// komorebi for Mac does. komorebi for Windows hides a window by cloaking it where it is by
    /// default, so there a hidden window is not told from a visible one by where it is, and the
    /// checks that rely on position are skipped rather than guessed. If this is `false`,
    /// `is_hidden_position` should be `false` as well.
    fn hides_windows_by_position(&self) -> bool;

    /// Reads the JSON komorebi sent a subscriber as an event, and returns an [`EventInfo`] that
    /// describes it.
    ///
    /// This never fails. JSON with no readable `type` that is not one of the shapes below is
    /// [`Family::Unknown`] and is named `unreadable`; a name that is not a window event or a
    /// monitor notification is read as a command, because a flavor has no list of commands.
    ///
    /// Three kinds reach a subscriber and look different on the wire. What differs by flavor:
    ///
    /// - komorebi for Mac: a window-manager event is `{"type": "MoveEnd", "content":
    ///   [{"type": "Accessibility", "content": "AXWindowMoved"}, <pid>, <window id>]}`, with a
    ///   process id and no program. A command is `{"type": "<Name>", "content": <argument>}`, with
    ///   no `content` when it has no argument, or a bare string. A monitor notification is a
    ///   one-entry object such as `{"DisplayConnectionChange": <display id>}`.
    /// - komorebi for Windows: a window event is `{"type": "FocusChange", "content": ["<WinEvent>",
    ///   <window>]}`, or `{"type": "Manage", "content": <window>}` for the events that have no
    ///   `WinEvent`. A window is `{"hwnd": ..., "title": ..., "exe": ..., "class": ..., "rect":
    ///   ...}`, so the event has a window handle and a program and no process id. A notification
    ///   about the machine is `{"type": "SessionLocked"}`; a virtual desktop notification is a bare
    ///   string, and any other bare string is [`Family::Unknown`], named after the string. A
    ///   command is `{"type": "<Name>", "content": <argument>}`, told apart from a window event
    ///   by its name.
    ///
    /// [`Family::Unknown`]: super::events::Family::Unknown
    fn describe_event(&self, event: &Value) -> EventInfo;

    /// Returns whether an event with this name starts or ends moving or resizing a window.
    ///
    /// komorebi for Mac names four: `MoveStart`, `MoveEnd`, `ResizeStart` and `ResizeEnd`.
    /// komorebi for Windows names two: `MoveResizeStart` and `MoveResizeEnd`.
    /// [`classify`](super::classify::classify) uses this to tell a button release that komorebi
    /// made up from the end of a real drag: one after which the layout changed.
    fn is_geometry_event(&self, name: &str) -> bool;

    /// Returns what a window-manager event (one the OS reported, as opposed to a command or a
    /// monitor change) was, as a [`Class`] and a sentence giving the reason.
    ///
    /// An event that starts or ends a move or resize (see
    /// [`is_geometry_event`](Rules::is_geometry_event)) is [`Class::Drag`] when the OS reported it,
    /// and [`Class::Manual`] when komorebi made it up. [`Class::Echo`] is not an answer to give:
    /// [`classify`](super::classify::classify) turns a drag that changed nothing, soon after
    /// komorebi acted, into an echo itself.
    fn classify_window_manager(&self, event: &EventRecord) -> (Class, &'static str);

    /// Returns whether `rect` is where komorebi parks a window it has hidden on `monitor`.
    ///
    /// `rect` is the window's rect as the `State` has it and `monitor` is the monitor the window is
    /// on. komorebi for Mac parks a hidden window with a one-point sliver still inside the
    /// display and the rules compare the horizontal position only, within 2 points. komorebi for
    /// Windows does not park windows, so its answer is always `false`.
    fn is_hidden_position(&self, rect: &Rect, monitor: &Monitor) -> bool;

    /// Returns the header of the handler's block that `line` starts, or `None` if `line` does not
    /// start one.
    ///
    /// `open` is the block that has been collecting lines and has not ended. komorebi for Mac
    /// logs a header line (`processing event: ...` or `processing command: ...`) and ignores
    /// `open`. komorebi for Windows has no header line: its lines carry a span such as
    /// `process_event{event="FocusChange" winevent="SystemForeground" hwnd=131072}`, and a new
    /// handler has begun when the span's fields differ from the open block's. Two consecutive
    /// handlers whose spans carry the same fields, with no end line between them, are therefore
    /// one block.
    fn block_header(&self, line: &Line, open: Option<&Block>) -> Option<BlockHeader>;

    /// Returns whether `line`, which belongs to a block of `kind`, is the one that marks the
    /// handler as done.
    ///
    /// It is asked of the lines that were added to an open block, not of the line that started
    /// it. A flavor whose log has no such line never ends a block that way. komorebi for Mac
    /// logs `processed command: ...` for a command and nothing for an event. komorebi for
    /// Windows logs `processed: <window>` for an event (at INFO, or TRACE for the noisiest
    /// ones) and `processed` for a command.
    fn is_block_end(&self, line: &Line, kind: BlockKind) -> bool;

    /// Returns what the part of komorebi that logs from `target` does.
    ///
    /// `target` is the module that logged, e.g. `komorebi::container`. The answer is for the
    /// longest module prefix in the flavor's table, which is `ROLES` in [`mac`] or [`windows`],
    /// and for a target with no such prefix the answer is `komorebi internals`.
    fn role(&self, target: &str) -> Role;

    /// Returns what a handler that took long was most likely doing, as a phrase that completes a
    /// sentence such as "took at least 250 ms to handle: ...".
    ///
    /// It names the calls the handler makes into other programs' windows, which differ by OS:
    /// accessibility calls on a Mac and Win32 calls on Windows. The phrase also names waiting for
    /// the lock another handler held.
    fn slow_handler_cause(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_with_no_flavor_recorded_is_komorebi_for_mac() {
        let info: crate::session::KomorebiInfo = serde_json::from_str(
            r#"{"running":false,"pid":null,"observed_levels":[],"data_dir":""}"#,
        )
        .expect("an old manifest's komorebi section");
        assert_eq!(info.flavor, Flavor::Macos);
    }

    #[test]
    fn flavors_are_spelled_in_lowercase_and_the_host_has_one() {
        assert_eq!(
            serde_json::to_string(&Flavor::Windows).expect("json"),
            r#""windows""#
        );
        assert_eq!(
            serde_json::from_str::<Flavor>(r#""macos""#).expect("parse"),
            Flavor::Macos
        );
        assert_eq!(Flavor::of_host() == Flavor::Windows, cfg!(windows));
    }

    #[test]
    fn each_flavor_has_rules_of_its_own_and_only_the_checked_ones_say_so() {
        assert_eq!(Flavor::Macos.rules().flavor(), Flavor::Macos);
        assert_eq!(Flavor::Windows.rules().flavor(), Flavor::Windows);
        assert!(Flavor::Macos.rules().verified());
        assert!(
            !Flavor::Windows.rules().verified(),
            "the Windows rules are from komorebi's source until a recording of it has been read"
        );
    }
}
