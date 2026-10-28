//! Which lines of komorebi's log belong to which event.
//!
//! komorebi handles one event or command at a time, under one lock, and everything it logs while
//! doing so carries the handler's span (`process_event:` or `process_command:`). That makes a
//! handler's output recoverable as a **block**: the header line (`processing event: ...`) and the
//! lines after it that share the span. Lines logged from other threads (the border manager, the
//! reaper, the reconcilers) carry no handler span; they are *ambient* and can only be related to a
//! handler by when they happened.
//!
//! Subscriber notifications are sent *after* the handler body, so an event's receive time marks the
//! **end** of its handler, not its start. [`pair`] ties each recorded event to the block that
//! produced it, and records how sure it is:
//!
//! - [`Quality::Pushed`]: komorebi logged `pushed notification to subscriber: <us>` (a DEBUG line)
//!   just before sending, which is exact.
//! - [`Quality::Header`]: matched by name, process and notification to the last header before the
//!   event was received, after the previous event.
//! - [`Quality::None`]: no block found. Events whose header is only logged at TRACE (the manual
//!   `Show` hacks) are like this, as are events recorded while komorebi's log was off.
//!
//! What cannot be known from the log is when komorebi *dequeued* the event: the time spent waiting
//! for the lock or for the accessibility calls made before the header is invisible.
//!
//! Where a block starts and ends depends on the komorebi that wrote the log, so the [`Rules`] of
//! its flavor decide: komorebi for Mac logs a header line, and komorebi for Windows is recognized
//! by the fields of the span its lines carry (see [`Rules::block_header`] and
//! [`Rules::is_block_end`]). This module is the same for both: [`build`] groups the lines, and
//! [`pair`] ties events to the groups.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::analysis::blocks::{BlockKind, build};
//! use komorebi_recorder::analysis::flavor::Flavor;
//! use komorebi_recorder::analysis::logparse;
//! use komorebi_recorder::clock::{Anchor, ClockMap};
//!
//! let log = "\
//! 2026-10-26T23:09:16.100000Z  INFO process_event: komorebi::process_event: processing event: MoveEnd for process 10 with notification AXWindowMoved
//! 2026-10-26T23:09:16.150000Z  INFO komorebi::border_manager: invalidating border observer
//! 2026-10-26T23:09:16.200000Z  WARN process_event:update_focused_workspace: komorebi::workspace: failed to set window position: AXError::InvalidUIElement
//! 2026-10-26T23:09:17.000000Z  INFO process_event: komorebi::process_event: processing event: FocusChange for process 11 with notification AXApplicationActivated
//! ";
//! let lines = logparse::parse(log.as_bytes());
//!
//! // the log's wall clock and the master clock, at two anchors a minute apart
//! let wall_us = chrono::DateTime::parse_from_rfc3339("2026-10-26T23:09:16Z")?.timestamp_micros();
//! let clock = ClockMap::new(vec![
//!     Anchor { mach_ns: 1_000_000_000, wall_us },
//!     Anchor { mach_ns: 61_000_000_000, wall_us: wall_us + 60_000_000 },
//! ]);
//!
//! let blocks = build(&lines, &clock, Flavor::Macos.rules());
//! assert_eq!(blocks.len(), 2);
//!
//! let first = &blocks[0];
//! assert_eq!(first.kind, BlockKind::Event);
//! assert_eq!(first.name, "MoveEnd");
//! assert_eq!(first.pid, Some(10));
//! // the border manager's line 2 has no handler span: it is ambient, in no block
//! assert_eq!(first.lines, [1, 3]);
//! assert_eq!(first.warnings, 1);
//! assert!((first.logged_ms() - 100.0).abs() < 1.0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::events::EventRecord;
use super::events::Family;
use super::flavor::BlockHeader;
use super::flavor::Rules;
use super::logparse::Level;
use super::logparse::Line;
use super::logparse::Span;
use crate::clock::ClockMap;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;

/// The kind of thing a handler was handling: an event from the OS or a command.
///
/// Serialized in kebab case (`event`, `command`). The two are told apart by the root span of the
/// handler's lines: `process_event` or `process_command`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BlockKind {
    /// The handler of an event the OS reported (`process_event`).
    Event,
    /// The handler of a command from `komorebic` or a hotkey daemon (`process_command`).
    Command,
}

/// A line komorebi logged when it handed a notification to a subscriber.
///
/// komorebi logs it at DEBUG, as `pushed notification to subscriber: <name>`, just before it sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Push {
    /// The subscriber's name, as registered with komorebi.
    pub subscriber: String,
    /// The line's number in `komorebi.log`, counting from 1.
    pub line: u64,
    /// When the line was logged, in nanoseconds on the host's master monotonic clock (`mach_` is a
    /// historical spelling).
    pub mach_ns: u64,
}

/// One handler's lines of komorebi's log: its header and the lines after it that share its span.
///
/// Built by [`build`]. A block starts at the header line the flavor's rules recognize and goes on
/// while lines carry the handler's root span of the same kind, until the rules say it finished or
/// the next header starts another. Times are nanoseconds on the host's master monotonic clock
/// (`mach_` is a historical spelling), mapped from the log's wall-clock times through the
/// session's clock anchors.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Block {
    /// The block's position in the list [`build`] returned, counting from 0. [`Pair::block`] names
    /// a block by it.
    pub id: usize,
    /// Whether the handler was handling an event or a command.
    pub kind: BlockKind,
    /// `MoveEnd`, `FocusWorkspaceNumber`, ...
    pub name: String,
    /// The process the event was about, where the log gives one (komorebi for Mac does).
    pub pid: Option<i32>,
    /// The window the event was about, where komorebi's log gives one (komorebi for Windows does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_id: Option<u32>,
    /// The notification the OS reported, e.g. `AXWindowMoved`, where the log gives it; `None` for a
    /// command.
    pub notification: Option<String>,
    /// The number of the header line in `komorebi.log`, counting from 1.
    pub header_line: u64,
    /// When the header was logged: the earliest known moment of the handler.
    pub header_mach_ns: u64,
    /// The number of the last line of the block in `komorebi.log`.
    pub last_line: u64,
    /// When the handler's last line was logged.
    pub end_mach_ns: u64,
    /// Line numbers of everything the handler logged, header first.
    pub lines: Vec<u64>,
    /// The line saying the handler finished, for the flavors whose log has one (`processed command:`
    /// on a Mac, for commands only; `processed` and `processed: <window>` on Windows).
    pub finished_line: Option<u64>,
    /// The lines in which komorebi handed a notification to a subscriber, in order.
    pub pushes: Vec<Push>,
    /// How many lines of the block are at level ERROR.
    pub errors: u32,
    /// How many lines of the block are at level WARN.
    pub warnings: u32,
}

impl Block {
    /// Returns the milliseconds from the header to the handler's last line.
    ///
    /// This is how long the handler was logging, which is at most how long it ran; the time before
    /// the header and after the last line is not in the log. For how long the handler took, at
    /// least, see [`Pair::handler_ms`].
    #[must_use]
    pub fn logged_ms(&self) -> f64 {
        (self.end_mach_ns.saturating_sub(self.header_mach_ns)) as f64 / 1e6
    }
}

const PUSHED: &str = "pushed notification to subscriber: ";

/// Returns the handler blocks of a log, in the order their headers were logged. `lines` are in
/// file order.
///
/// What a block's first and last line look like is komorebi's own business, so the `rules` of the
/// flavor that wrote the log say; what they have in common is that everything logged inside a
/// handler carries the handler's root span. For each line that has a time:
///
/// - If [`Rules::block_header`] returns a header for the line, a new block starts, and the one that
///   was open is left as it was (it has no `finished_line`).
/// - Otherwise, a line whose root span is `process_event` or `process_command` joins the open block
///   when the kinds match: its number, time and level are added, and a `pushed notification` line
///   is recorded in `pushes`. When [`Rules::is_block_end`] returns `true` for it, the block is
///   closed and `finished_line` is set.
/// - Any other line is ambient and is in no block: one logged from another thread, one from a
///   handler of the other kind, one before the first header and one after the block ended.
///
/// Continuation lines (a panic's text) are not looked at. A line whose time cannot be mapped to the
/// master clock (the session has no clock anchors, or the line has no time) is skipped.
///
/// The list is empty when the log has no handler headers.
///
/// See the [module documentation](self) for an example.
#[must_use]
pub fn build(lines: &[Line], clock: &ClockMap, rules: &dyn Rules) -> Vec<Block> {
    let mach = |line: &Line| line.wall_us.and_then(|wall_us| clock.wall_to_mach(wall_us));

    let mut blocks: Vec<Block> = Vec::new();
    let mut open: Option<usize> = None;
    // the root span a command block was opened with, to tell two commands of the same name apart
    let mut command_span: Option<&Span> = None;

    for line in lines.iter().filter(|line| !line.continuation) {
        let Some(time) = mach(line) else {
            continue;
        };

        let mut header = rules.block_header(line, open.map(|index| &blocks[index]));
        // a command that failed leaves its block open, and the next command of the same name, with
        // other arguments, is another handler
        if header.is_none()
            && line.root_span() == Some("process_command")
            && open.is_some_and(|index| blocks[index].kind == BlockKind::Command)
            && command_span.is_some_and(|span| line.spans.first() != Some(span))
        {
            header = rules.block_header(line, None);
        }
        if let Some(header) = header {
            if header.kind == BlockKind::Command {
                command_span = line.spans.first();
            }
            blocks.push(new_block(blocks.len(), header, line.number, time));
            open = Some(blocks.len() - 1);
            continue;
        }

        let Some(root @ ("process_event" | "process_command")) = line.root_span() else {
            continue;
        };
        let Some(index) = open else {
            continue;
        };
        let block = &mut blocks[index];
        let belongs = matches!(
            (root, block.kind),
            ("process_event", BlockKind::Event) | ("process_command", BlockKind::Command)
        );
        if !belongs {
            continue;
        }

        block.lines.push(line.number);
        block.last_line = line.number;
        block.end_mach_ns = time;
        match line.level {
            Some(Level::Error) => block.errors += 1,
            Some(Level::Warn) => block.warnings += 1,
            _ => {}
        }

        if let Some(subscriber) = line.message.strip_prefix(PUSHED) {
            block.pushes.push(Push {
                subscriber: subscriber.trim().to_string(),
                line: line.number,
                mach_ns: time,
            });
        }

        if rules.is_block_end(line, block.kind) {
            block.finished_line = Some(line.number);
            open = None;
        }
    }

    blocks
}

fn new_block(id: usize, header: BlockHeader, line: u64, time: u64) -> Block {
    Block {
        id,
        kind: header.kind,
        name: header.name,
        pid: header.pid,
        window_id: header.window_id,
        notification: header.notification,
        header_line: line,
        header_mach_ns: time,
        last_line: line,
        end_mach_ns: time,
        lines: vec![line],
        finished_line: None,
        pushes: Vec::new(),
        errors: 0,
        warnings: 0,
    }
}

/// How sure the pairing of an event with a handler is.
///
/// Serialized in kebab case (`pushed`, `header`, `none`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Quality {
    /// komorebi logged handing the notification to this subscriber.
    Pushed,
    /// Matched by name, process and notification.
    Header,
    /// No block was found.
    None,
}

/// An event tied to the handler block that produced it, or to none.
///
/// Made by [`pair`], one for each event.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Pair {
    /// The event's sequence number.
    pub seq: u64,
    /// The [`Block::id`] of the block that produced the event, or `None` if none was found.
    pub block: Option<usize>,
    /// How sure the pairing is. [`Quality::None`] exactly when `block` is `None`.
    pub quality: Quality,
    /// The `pushed notification` line for this event, if there was one.
    pub pushed_line: Option<u64>,
    /// When komorebi logged handing this notification to the recorder, on the master clock in
    /// nanoseconds: komorebi's own send time, as opposed to the receive time in `events.jsonl`.
    /// `None` when the log has no such line for this event.
    #[serde(default)]
    pub pushed_mach_ns: Option<u64>,
    /// Milliseconds from komorebi sending the notification (`pushed_mach_ns`) to the recorder
    /// receiving it. Near 0 normally; a large value means the notification sat in a socket or a
    /// queue, which is delivery delay and not komorebi's doing. `None` when the send time is not
    /// known.
    #[serde(default)]
    pub delivery_ms: Option<f64>,
    /// Milliseconds the handler took, at least. From its header to the push when the push is known
    /// and was not received at once (so that delivery delay is not counted as handling), otherwise
    /// from its header to the event being received.
    pub handler_ms: Option<f64>,
}

/// How far a log time may be from the recorder's own clock reading and still be taken to be the
/// same moment. The two are mapped through the clock anchors, which are accurate to about a
/// millisecond; the margin is for scheduling in between.
const SLACK_NS: u64 = 25_000_000;

/// How close a `pushed notification` line must be before an event's receipt.
const PUSH_WINDOW_NS: u64 = 100_000_000;

/// The longest a notification is believed to sit between komorebi sending it and the recorder
/// receiving it, when lining pushes up with events in order. A push further back than this is not
/// taken for the event's.
const MAX_DELIVERY_NS: u64 = 30_000_000_000;

/// How many pushes may be skipped to find the one an event matches, when lining them up in order
/// (a notification the recorder did not keep leaves one over).
const ALIGN_LOOKAHEAD: usize = 3;

/// Lines komorebi's pushes to `subscriber` up with the events, in order.
///
/// komorebi sends to a subscriber over one socket, so the notifications arrive in the order they
/// were pushed: the first event received is the first push logged, and so on. That identifies the
/// push, and so the handler, of an event even when it was received long after it was sent, which
/// the 100 ms window of [`Quality::Pushed`] cannot. A push is taken for an event when it was
/// logged no later than the event was received (with slack), no more than 30 s before it, and its
/// block has the event's name; up to three pushes may be skipped to get there. Returns, for each
/// event, the block id and the push, or `None`.
fn align<'a>(
    events: &[EventRecord],
    blocks: &'a [Block],
    subscriber: Option<&str>,
) -> Vec<Option<(usize, &'a Push)>> {
    let Some(name) = subscriber else {
        return vec![None; events.len()];
    };

    let mut pushes: Vec<(usize, &Push)> = blocks
        .iter()
        .flat_map(|block| block.pushes.iter().map(move |push| (block.id, push)))
        .filter(|(_, push)| push.subscriber == name)
        .collect();
    pushes.sort_by_key(|(_, push)| push.line);

    let mut next = 0;
    events
        .iter()
        .map(|event| {
            let found = (next..pushes.len().min(next + 1 + ALIGN_LOOKAHEAD)).find(|index| {
                let (block, push) = pushes[*index];
                push.mach_ns <= event.recv_mach_ns.saturating_add(SLACK_NS)
                    && event.recv_mach_ns.saturating_sub(push.mach_ns) <= MAX_DELIVERY_NS
                    && blocks[block].name == event.info.name
            });
            found.map(|index| {
                next = index + 1;
                pushes[index]
            })
        })
        .collect()
}

/// Returns the subscriber name the recorder registered as, found from which `pushed notification`
/// lines line up with the recorded events, or `None` if none does.
///
/// komorebi logs the name of each subscriber it pushes a notification to. The recorder is the one
/// whose pushes come just before the events it recorded: a push counts for an event if it was
/// logged from 100 ms before the event was received to 25 ms after (the margin is for the clock
/// mapping and scheduling). The name with the most events lined up wins, and where two tie, a name
/// that starts `komorebi-recorder-` is preferred.
///
/// A session records the name it registered as, which is better evidence; this is for the
/// sessions that do not. See [`pair`] for an example.
#[must_use]
pub fn infer_subscriber(blocks: &[Block], events: &[EventRecord]) -> Option<String> {
    let mut pushes: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for push in blocks.iter().flat_map(|block| &block.pushes) {
        pushes
            .entry(push.subscriber.as_str())
            .or_default()
            .push(push.mach_ns);
    }

    let score = |times: &[u64]| {
        events
            .iter()
            .filter(|event| {
                times.iter().any(|time| {
                    *time <= event.recv_mach_ns + SLACK_NS
                        && event.recv_mach_ns <= *time + PUSH_WINDOW_NS
                })
            })
            .count()
    };

    pushes
        .into_iter()
        .map(|(name, times)| {
            // our own subscriber names start with this; prefer them on a tie
            let ours = name.starts_with("komorebi-recorder-");
            (score(&times), ours, name)
        })
        .filter(|(score, _, _)| *score > 0)
        .max_by_key(|(score, ours, _)| (*score, *ours))
        .map(|(_, _, name)| name.to_string())
}

/// The line where komorebi handed `block`'s notification to `subscriber`, if the log has it.
fn push_for<'a>(
    block: &'a Block,
    subscriber: Option<&str>,
    recv_mach_ns: u64,
    upper: u64,
) -> Option<&'a Push> {
    let name = subscriber?;
    block
        .pushes
        .iter()
        .filter(|push| push.subscriber == name)
        .filter(|push| push.mach_ns <= upper && recv_mach_ns <= push.mach_ns + PUSH_WINDOW_NS)
        .max_by_key(|push| push.mach_ns)
}

/// Returns, for each event, the block that produced it and how sure the pairing is.
///
/// There is one [`Pair`] per event, in the same order. `events` must be in receipt order, which is
/// how [`events::load`](super::events::load) returns them.
///
/// Each event is matched to a block in turn. The candidates are the blocks that no earlier event
/// took, of the kind the event's family has (a window-manager event pairs with an event block, a
/// command with a command block, and a monitor notification or an unreadable one with none), with
/// the same name, whose header was logged after the previous event was received and no later than
/// this one was (each with 25 ms of slack), and whose process, window handle and notification each
/// agree wherever both the block and the event have one. Of those, the block with the latest header
/// is taken: the handler that finished last before the notification. A block is paired with at most
/// one event.
///
/// The pairing is [`Quality::Pushed`] when `subscriber` is given and the block has a
/// `pushed notification to subscriber:` line for it, logged from 100 ms before to 25 ms after the
/// event was received. Without a `subscriber`, a found block is [`Quality::Header`]. With no
/// block, the pair is [`Quality::None`] and has neither a block nor a handler time. A pairing is a
/// judgement from timing, and only a `Pushed` one is exact.
///
/// With a `subscriber`, the pushes to it are also lined up with the events in order, because
/// komorebi sends over one socket and the notifications arrive in the order they were pushed. That
/// pairs an event with its handler even when the recorder received it long after komorebi sent it
/// (notifications are sometimes delivered in clumps), and gives [`Pair::pushed_mach_ns`] and
/// [`Pair::delivery_ms`]. Such a pair's `handler_ms` ends at the push, not at the receipt, so that
/// delivery delay is not counted as handling.
///
/// # Performance
///
/// Each event looks at every block, so the cost grows with events times blocks.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::blocks::{Quality, build, infer_subscriber, pair};
/// use komorebi_recorder::analysis::events::EventRecord;
/// use komorebi_recorder::analysis::flavor::Flavor;
/// use komorebi_recorder::analysis::logparse;
/// use komorebi_recorder::clock::{Anchor, ClockMap};
/// use serde_json::json;
///
/// let log = "\
/// 2026-10-26T23:09:16.100000Z  INFO process_event: komorebi::process_event: processing event: MoveEnd for process 10 with notification AXWindowMoved
/// 2026-10-26T23:09:16.250000Z DEBUG process_event: komorebi::notify: pushed notification to subscriber: komorebi-recorder-4242
/// ";
/// let lines = logparse::parse(log.as_bytes());
/// let wall_us = chrono::DateTime::parse_from_rfc3339("2026-10-26T23:09:16Z")?.timestamp_micros();
/// let clock = ClockMap::new(vec![
///     Anchor { mach_ns: 1_000_000_000, wall_us },
///     Anchor { mach_ns: 61_000_000_000, wall_us: wall_us + 60_000_000 },
/// ]);
/// let blocks = build(&lines, &clock, Flavor::Macos.rules());
///
/// // the recorder received the notification 250.5 ms after the clock's anchor
/// let moved = json!({
///     "type": "MoveEnd",
///     "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, 10, 7]
/// });
/// let events = vec![EventRecord {
///     seq: 1,
///     recv_mach_ns: 1_250_500_000,
///     recv_wall_us: 0,
///     state_id: None,
///     info: Flavor::Macos.rules().describe_event(&moved),
///     parse_error: None,
///     probe: None,
/// }];
///
/// // the log names the subscriber the push went to, so the pairing is exact
/// let subscriber = infer_subscriber(&blocks, &events);
/// assert_eq!(subscriber.as_deref(), Some("komorebi-recorder-4242"));
///
/// let pairs = pair(&events, &blocks, subscriber.as_deref());
/// assert_eq!(pairs[0].block, Some(0));
/// assert_eq!(pairs[0].quality, Quality::Pushed);
/// assert_eq!(pairs[0].pushed_line, Some(2));
/// // from the header (1.1 s) to the receipt (1.2505 s)
/// assert!((pairs[0].handler_ms.expect("a handler") - 150.5).abs() < 0.01);
///
/// // without the subscriber's name it is matched by name, process and notification only
/// assert_eq!(pair(&events, &blocks, None)[0].quality, Quality::Header);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn pair(events: &[EventRecord], blocks: &[Block], subscriber: Option<&str>) -> Vec<Pair> {
    let mut used = vec![false; blocks.len()];
    let mut previous_recv = 0_u64;
    let mut pairs = Vec::with_capacity(events.len());

    // the pushes that line up with an event in order claim their blocks first
    let aligned = align(events, blocks, subscriber);
    for (block, _) in aligned.iter().flatten() {
        used[*block] = true;
    }

    for (event, aligned) in events.iter().zip(aligned) {
        let upper = event.recv_mach_ns.saturating_add(SLACK_NS);
        let lower = previous_recv.saturating_sub(SLACK_NS);

        if let Some((id, push)) = aligned {
            let delivery_ns = event.recv_mach_ns.saturating_sub(push.mach_ns);
            // a notification received long after it was sent says nothing about the handler: count
            // the handler until komorebi sent it
            let until = if delivery_ns > PUSH_WINDOW_NS {
                push.mach_ns
            } else {
                event.recv_mach_ns
            };
            pairs.push(Pair {
                seq: event.seq,
                block: Some(id),
                quality: Quality::Pushed,
                pushed_line: Some(push.line),
                pushed_mach_ns: Some(push.mach_ns),
                delivery_ms: Some(delivery_ns as f64 / 1e6),
                handler_ms: Some(until.saturating_sub(blocks[id].header_mach_ns) as f64 / 1e6),
            });
            previous_recv = event.recv_mach_ns;
            continue;
        }

        let wanted = match event.info.family {
            Family::WindowManager => Some(BlockKind::Event),
            Family::Command => Some(BlockKind::Command),
            Family::Monitor | Family::Unknown => None,
        };

        let candidate = wanted.and_then(|kind| {
            blocks
                .iter()
                .filter(|block| {
                    !used[block.id]
                        && block.kind == kind
                        && block.name == event.info.name
                        && block.header_mach_ns > lower
                        && block.header_mach_ns <= upper
                        && match (&block.pid, &event.info.pid) {
                            (Some(a), Some(b)) => a == b,
                            _ => true,
                        }
                        && match (&block.window_id, &event.info.window_id) {
                            (Some(a), Some(b)) => a == b,
                            _ => true,
                        }
                        && match (&block.notification, &event.info.notification) {
                            (Some(a), Some(b)) => a == b,
                            _ => true,
                        }
                })
                // the handler that finished last before the notification
                .max_by_key(|block| block.header_mach_ns)
        });

        let pair = if let Some(block) = candidate {
            used[block.id] = true;
            let push = push_for(block, subscriber, event.recv_mach_ns, upper);
            Pair {
                seq: event.seq,
                block: Some(block.id),
                quality: if push.is_some() {
                    Quality::Pushed
                } else {
                    Quality::Header
                },
                pushed_line: push.map(|push| push.line),
                pushed_mach_ns: push.map(|push| push.mach_ns),
                delivery_ms: push
                    .map(|push| event.recv_mach_ns.saturating_sub(push.mach_ns) as f64 / 1e6),
                handler_ms: Some(
                    event.recv_mach_ns.saturating_sub(block.header_mach_ns) as f64 / 1e6,
                ),
            }
        } else {
            Pair {
                seq: event.seq,
                block: None,
                quality: Quality::None,
                pushed_line: None,
                pushed_mach_ns: None,
                delivery_ms: None,
                handler_ms: None,
            }
        };

        pairs.push(pair);
        previous_recv = event.recv_mach_ns;
    }

    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::flavor::Flavor;
    use crate::analysis::logparse;
    use crate::clock::Anchor;
    use crate::testkit;
    use serde_json::json;

    /// A clock where wall time is mach time plus the testkit's constant.
    fn clock() -> ClockMap {
        ClockMap::new(vec![
            Anchor {
                mach_ns: testkit::mach_ns(-1.0),
                wall_us: testkit::wall_us(-1.0),
            },
            Anchor {
                mach_ns: testkit::mach_ns(60.0),
                wall_us: testkit::wall_us(60.0),
            },
        ])
    }

    /// Log text from `(video_s, level, text-after-level)` lines, like komorebi's.
    fn log(lines: &[(f64, &str, &str)]) -> Vec<Line> {
        logparse::parse(testkit::log_text(lines).as_bytes())
    }

    fn event(seq: u64, video_s: f64, value: serde_json::Value) -> EventRecord {
        EventRecord {
            seq,
            recv_mach_ns: testkit::mach_ns(video_s),
            recv_wall_us: testkit::wall_us(video_s),
            state_id: None,
            info: Flavor::Macos.rules().describe_event(&value),
            parse_error: None,
            probe: None,
        }
    }

    fn move_end(pid: i64) -> serde_json::Value {
        json!({"type": "MoveEnd", "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, pid, 7]})
    }

    const HEADER: &str = "process_event: komorebi::process_event: processing event: MoveEnd for process 10 with notification AXWindowMoved";

    #[test]
    fn a_handlers_lines_are_everything_with_its_span_until_the_next_header() {
        let lines = log(&[
            (1.0, "INFO", HEADER),
            (
                1.1,
                "INFO",
                "process_event:focus_window{idx=0}: komorebi::container: focusing window",
            ),
            (
                1.15,
                "INFO",
                "komorebi::border_manager: invalidating border observer",
            ),
            (
                1.2,
                "WARN",
                "process_event:update_focused_workspace: komorebi::workspace: failed to set window position: AXError::InvalidUIElement",
            ),
            (
                2.0,
                "INFO",
                "process_event: komorebi::process_event: processing event: FocusChange for process 11 with notification AXApplicationActivated",
            ),
        ]);

        let blocks = build(&lines, &clock(), Flavor::Macos.rules());
        assert_eq!(blocks.len(), 2);

        let first = &blocks[0];
        assert_eq!(first.kind, BlockKind::Event);
        assert_eq!(first.name, "MoveEnd");
        assert_eq!(first.pid, Some(10));
        assert_eq!(first.notification.as_deref(), Some("AXWindowMoved"));
        // the border manager's line is ambient: logged from another thread, no handler span
        assert_eq!(first.lines, [1, 2, 4]);
        assert_eq!(first.warnings, 1);
        assert!((first.logged_ms() - 200.0).abs() < 1.0);

        assert_eq!(blocks[1].name, "FocusChange");
        assert_eq!(blocks[1].lines, [5]);
    }

    #[test]
    fn a_command_block_ends_at_its_processed_line() {
        let lines = log(&[
            (
                1.0,
                "INFO",
                "process_command: komorebi::process_command: processing command: FocusWorkspaceNumber",
            ),
            (
                1.1,
                "INFO",
                "process_command:focus_workspace{idx=0}: komorebi::window_manager: focusing workspace",
            ),
            (
                1.3,
                "INFO",
                "process_command: komorebi::process_command: processed command: FocusWorkspaceNumber",
            ),
            // after the command ended: ambient again
            (
                1.4,
                "INFO",
                "process_command:something: komorebi::x: late line",
            ),
        ]);

        let blocks = build(&lines, &clock(), Flavor::Macos.rules());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Command);
        assert_eq!(blocks[0].name, "FocusWorkspaceNumber");
        assert_eq!(blocks[0].finished_line, Some(3));
        assert_eq!(blocks[0].lines, [1, 2, 3]);
    }

    #[test]
    fn handler_lines_before_any_header_belong_to_no_block() {
        let lines = log(&[
            (
                0.5,
                "DEBUG",
                "process_event: komorebi::process_event: ignoring event as window should not be managed: X",
            ),
            (1.0, "INFO", HEADER),
        ]);

        let blocks = build(&lines, &clock(), Flavor::Macos.rules());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lines, [2]);
    }

    #[test]
    fn an_event_pairs_with_the_last_matching_header_before_it_was_received() {
        let lines = log(&[(1.0, "INFO", HEADER), (3.0, "INFO", HEADER)]);
        let blocks = build(&lines, &clock(), Flavor::Macos.rules());
        // received at 3.25s: the handler that began at 3.0s sent it; the one at 1.0s never did
        let events = vec![event(1, 3.25, move_end(10))];

        let pairs = pair(&events, &blocks, None);
        assert_eq!(pairs[0].block, Some(1));
        assert_eq!(pairs[0].quality, Quality::Header);
        assert!((pairs[0].handler_ms.expect("ms") - 250.0).abs() < 1.0);
    }

    #[test]
    fn an_event_is_not_paired_across_another_event() {
        let lines = log(&[(1.0, "INFO", HEADER)]);
        let blocks = build(&lines, &clock(), Flavor::Macos.rules());
        let events = vec![
            event(1, 1.1, move_end(10)),
            // a second event of the same kind with no handler logged: it must not steal block 0
            event(2, 5.0, move_end(10)),
        ];

        let pairs = pair(&events, &blocks, None);
        assert_eq!(pairs[0].block, Some(0));
        assert_eq!(pairs[1].block, None);
        assert_eq!(pairs[1].quality, Quality::None);
    }

    #[test]
    fn name_process_and_notification_must_agree() {
        let lines = log(&[(1.0, "INFO", HEADER)]);
        let blocks = build(&lines, &clock(), Flavor::Macos.rules());

        let other_process = pair(&[event(1, 1.1, move_end(99))], &blocks, None);
        assert_eq!(other_process[0].block, None);

        let other_name = pair(
            &[event(
                1,
                1.1,
                json!({"type": "ResizeEnd", "content": [{"type": "Accessibility", "content": "AXWindowMoved"}, 10, 7]}),
            )],
            &blocks,
            None,
        );
        assert_eq!(other_name[0].block, None);
    }

    #[test]
    fn a_pushed_line_makes_the_pairing_exact_and_names_our_subscriber() {
        let lines = log(&[
            (1.0, "INFO", HEADER),
            (
                1.2,
                "DEBUG",
                "process_event: komorebi::notify: pushed notification to subscriber: komorebi-bar-x",
            ),
            (
                1.2001,
                "DEBUG",
                "process_event: komorebi::notify: pushed notification to subscriber: komorebi-recorder-4242",
            ),
        ]);
        let blocks = build(&lines, &clock(), Flavor::Macos.rules());
        let events = vec![event(1, 1.2005, move_end(10))];

        assert_eq!(
            infer_subscriber(&blocks, &events).as_deref(),
            Some("komorebi-recorder-4242")
        );

        let pairs = pair(&events, &blocks, Some("komorebi-recorder-4242"));
        assert_eq!(pairs[0].quality, Quality::Pushed);
        assert_eq!(pairs[0].pushed_line, Some(3));
    }

    #[test]
    fn commands_and_monitor_events_pair_by_family() {
        let lines = log(&[
            (
                1.0,
                "INFO",
                "process_command: komorebi::process_command: processing command: Retile",
            ),
            (
                1.2,
                "INFO",
                "process_command: komorebi::process_command: processed command: Retile",
            ),
        ]);
        let blocks = build(&lines, &clock(), Flavor::Macos.rules());
        let events = vec![
            event(1, 1.1, json!({"type": "Retile"})),
            event(2, 3.0, json!({"DisplayConnectionChange": 1})),
        ];

        let pairs = pair(&events, &blocks, None);
        assert_eq!(pairs[0].block, Some(0));
        assert_eq!(pairs[1].block, None);
    }

    /// Run with `--ignored` on a machine that runs komorebi: its real log must group into handler
    /// blocks, and nearly every line must be found in the source.
    #[test]
    #[ignore = "reads the real komorebi log of the machine"]
    fn the_real_log_groups_into_handlers() {
        let dir = crate::komorebi_link::data_dir().to_path_buf();
        let newest = std::fs::read_dir(&dir)
            .expect("data dir")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(crate::sources::komorebi_log::PLAINTEXT_PREFIX)
            })
            .max_by_key(|entry| entry.file_name())
            .expect("a plaintext log");
        let bytes = std::fs::read(newest.path()).expect("read");
        let lines = logparse::parse(&bytes);

        // wall time is mach time plus a constant, so any anchors will do
        let first = lines
            .iter()
            .find_map(|line| line.wall_us)
            .expect("a timestamp");
        let clock = ClockMap::new(vec![
            Anchor {
                mach_ns: 1_000_000_000,
                wall_us: first,
            },
            Anchor {
                mach_ns: 1_000_000_000 + 86_400_000_000_000,
                wall_us: first + 86_400_000_000,
            },
        ]);
        let blocks = build(&lines, &clock, Flavor::Macos.rules());

        let events = blocks
            .iter()
            .filter(|block| block.kind == BlockKind::Event)
            .count();
        let commands = blocks
            .iter()
            .filter(|block| block.kind == BlockKind::Command)
            .count();
        let in_blocks: usize = blocks.iter().map(|block| block.lines.len()).sum();
        let handler_lines = lines
            .iter()
            .filter(|line| matches!(line.root_span(), Some("process_event" | "process_command")))
            .count();
        let found = lines
            .iter()
            .filter(|line| !line.continuation)
            .filter(|line| {
                matches!((&line.target, line.level), (Some(target), Some(level))
                    if crate::analysis::catalog::of(Flavor::Macos).find(target, level, &line.message).is_some())
            })
            .count();
        let structured = lines.iter().filter(|line| !line.continuation).count();

        eprintln!(
            "{} lines: {events} event blocks, {commands} command blocks holding {in_blocks} of {handler_lines} handler lines; \
             {found} of {structured} lines found in the source ({}%)",
            lines.len(),
            found * 100 / structured.max(1)
        );
        assert!(events > 0);
        assert!(
            in_blocks * 10 >= handler_lines * 7,
            "most handler lines are in a block"
        );
    }

    #[test]
    fn notifications_received_late_are_lined_up_with_their_pushes_in_order() {
        let push =
            "process_event: komorebi: pushed notification to subscriber: komorebi-recorder-1";
        let lines = log(&[
            (1.0, "INFO", HEADER),
            (1.005, "DEBUG", push),
            (2.0, "INFO", HEADER),
            (2.005, "DEBUG", push),
        ]);
        let blocks = build(&lines, &clock(), Flavor::Macos.rules());
        // both are received together at 5 s: the second block is not "the latest before receipt"
        let events = vec![event(1, 5.000, move_end(10)), event(2, 5.001, move_end(10))];

        let pairs = pair(&events, &blocks, Some("komorebi-recorder-1"));
        assert_eq!(pairs[0].block, Some(0));
        assert_eq!(pairs[1].block, Some(1));
        for pair in &pairs {
            assert_eq!(pair.quality, Quality::Pushed);
            assert!((pair.handler_ms.expect("handler") - 5.0).abs() < 0.01);
        }
        assert!((pairs[0].delivery_ms.expect("delivery") - 3995.0).abs() < 0.01);
        assert_eq!(pairs[1].pushed_mach_ns, Some(testkit::mach_ns(2.005)));

        // without the subscriber, nothing says when komorebi sent them
        let unpushed = pair(&events, &blocks, None);
        assert_eq!(unpushed[0].quality, Quality::Header);
        assert_eq!(unpushed[0].pushed_mach_ns, None);
    }

    #[test]
    fn a_push_for_an_event_that_was_not_recorded_is_skipped() {
        let push =
            "process_event: komorebi: pushed notification to subscriber: komorebi-recorder-1";
        let focus = "process_event: komorebi::process_event: processing event: FocusChange for process 11 with notification AXApplicationActivated";
        let lines = log(&[
            (1.0, "INFO", focus),
            (1.005, "DEBUG", push),
            (2.0, "INFO", HEADER),
            (2.005, "DEBUG", push),
        ]);
        let blocks = build(&lines, &clock(), Flavor::Macos.rules());
        // the recorder missed the first notification
        let events = vec![event(1, 2.01, move_end(10))];

        let pairs = pair(&events, &blocks, Some("komorebi-recorder-1"));
        assert_eq!(pairs[0].block, Some(1));
        assert_eq!(pairs[0].quality, Quality::Pushed);
    }
}
