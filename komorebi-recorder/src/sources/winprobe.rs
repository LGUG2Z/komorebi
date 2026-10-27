//! What Windows itself says about a window at the moment komorebi reports an event about it
//! (`record --probe-windows`).
//!
//! komorebi for Windows hides a window by *cloaking* it where it is, so its `State` cannot say
//! whether a window is really hidden: a window on a workspace that is not showing has the same rect
//! as when it was. The only way to know is to ask Windows. With the probe on, each window event is
//! followed by a read of the window's real state (is it visible, cloaked, minimized, and where is
//! it), stored with the event, and sometimes by komorebi's own answer to "would you manage this
//! window, and why". The analysis then checks the two against each other
//! ([`crate::analysis::invariants::check_probed`]): a window komorebi shows that Windows has
//! hidden, one it hides that Windows shows, and a visible window it does not manage.
//!
//! The probe runs on the thread that writes events, never the one that accepts them: komorebi
//! delivers a notification while holding its own locks, and a probe that asked komorebi something
//! while the accept thread was stuck would wait on a lock that is waiting on it.
//!
//! # What is written
//!
//! No file of its own. A probe is stored as the `probe` key of the `events.jsonl` row of the event
//! it is about ([`EventRow::probe`](crate::sources::komorebi_events::EventRow::probe)), as a
//! [`WindowFacts`]. A row has a probe only for a window event, and only in a recording made with
//! `--probe-windows`; every other row, and every row of any other recording, has none.
//!
//! # Host and flavor
//!
//! The prober reads komorebi for Windows' events (it asks the [`Flavor::Windows`] rules which
//! events are about which window) and Windows' window state, so it is meaningful only for a
//! recording made on Windows. The module builds on every host, because the types are read from
//! sessions anywhere, but on any other host [`EventProber::windows`] has nothing to ask
//! ([`crate::platform::window::inspect`] returns `None`), and `record --probe-windows` is refused
//! there.

use crate::analysis::events::Family;
use crate::analysis::flavor::Flavor;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// komorebi's answers are kept to this much of the start: they list the rules that matched, which
/// is what matters, and not every attribute of the window.
const SAYS_LIMIT: usize = 2_000;

/// A window komorebi is asked about again no sooner than this.
const ASK_AGAIN_NS: u64 = 5_000_000_000;

/// How many windows are remembered as asked about before the memory is cleared.
const ASKED_CAPACITY: usize = 4096;

/// Events after which komorebi's decision about a window may have changed, so it is asked again.
const DECIDING_EVENTS: [&str; 5] = ["Show", "Manage", "Unmanage", "Uncloak", "TitleUpdate"];

/// What Windows said about one window, read just after komorebi reported an event about it.
///
/// This is the value of the `probe` key of an `events.jsonl` row. Every field is always written,
/// except `komorebi_says`, which is left out when absent.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::winprobe::WindowFacts;
///
/// let facts = WindowFacts {
///     hwnd: 131_072,
///     at_ns: 5_000_000_000,
///     exists: true,
///     visible: true,
///     cloaked: true,
///     minimized: false,
///     rect: Some([0, 0, 1920, 1080]),
///     komorebi_says: None,
/// };
///
/// // a cloaked window is how komorebi hides one: it is still `visible`, and not shown
/// assert!(!facts.shown());
/// assert_eq!(facts.hidden_how(), Some("cloaked"));
///
/// let text = serde_json::to_string(&facts)?;
/// assert!(text.contains(r#""rect":[0,0,1920,1080]"#) && !text.contains("komorebi_says"));
/// assert_eq!(serde_json::from_str::<WindowFacts>(&text)?, facts);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WindowFacts {
    /// The window handle, the same number komorebi for Windows gives the window as its `hwnd`.
    pub hwnd: u32,
    /// When it was read, in nanoseconds on the master clock: later than the event, by however
    /// long the event waited to be written.
    pub at_ns: u64,
    /// The handle still names a window (it does not once the window is destroyed).
    pub exists: bool,
    /// `IsWindowVisible`. A window that komorebi has hidden by cloaking it is still visible in
    /// this sense: see `cloaked`.
    pub visible: bool,
    /// Cloaked by the shell (on another virtual desktop), by an application, or by komorebi,
    /// which is how komorebi for Windows hides a window.
    pub cloaked: bool,
    /// `IsIconic`.
    pub minimized: bool,
    /// Where Windows has it, as komorebi reads a window's rect (the visible frame): `[left, top,
    /// right, bottom]` in physical pixels. `None` if the window does not exist or its rect could
    /// not be read.
    pub rect: Option<[i32; 4]>,
    /// komorebi's own answer to "would you manage this window, and which rules matched", verbatim
    /// and cut short to its first 2,000 characters. Only asked now and then, so usually absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub komorebi_says: Option<String>,
}

impl WindowFacts {
    /// Returns `true` if and only if the window is on screen to a person: it exists, is visible,
    /// and is neither cloaked nor minimized.
    #[must_use]
    pub fn shown(&self) -> bool {
        self.exists && self.visible && !self.cloaked && !self.minimized
    }

    /// Returns how the window is hidden, in words, if it is: `destroyed`, `cloaked`, `minimized`
    /// or `not visible`, the first that applies in that order. `None` for a window that is not
    /// hidden by any of them.
    #[must_use]
    pub fn hidden_how(&self) -> Option<&'static str> {
        if !self.exists {
            Some("destroyed")
        } else if self.cloaked {
            Some("cloaked")
        } else if self.minimized {
            Some("minimized")
        } else if !self.visible {
            Some("not visible")
        } else {
            None
        }
    }
}

/// When komorebi is asked about a window: the first time it is seen, and again after an event that
/// can change the answer, but never in a flurry.
///
/// The events that can change the answer are `Show`, `Manage`, `Unmanage`, `Uncloak` and
/// `TitleUpdate`, and a window is asked about again after one of them only if the last time was
/// at least five seconds before. The policy remembers up to 4096 windows and forgets all of them
/// when that fills, so a window is then asked about as if it were new.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::winprobe::AskPolicy;
///
/// let mut policy = AskPolicy::default();
/// let second = 1_000_000_000;
///
/// assert!(policy.should_ask(7, "FocusChange", second), "the first time a window is seen");
/// assert!(!policy.should_ask(7, "FocusChange", 60 * second), "focus does not change komorebi's mind");
/// assert!(policy.should_ask(7, "Show", 61 * second), "a Show after the quiet period can");
/// assert!(!policy.should_ask(7, "Show", 62 * second), "but not in a flurry");
/// assert!(policy.should_ask(8, "FocusChange", 62 * second), "another window is its own first time");
/// ```
#[derive(Debug, Default)]
pub struct AskPolicy {
    asked: HashMap<u32, u64>,
}

impl AskPolicy {
    /// Returns `true` if komorebi should be asked about window `hwnd` now, after an event named
    /// `event` heard at `now_ns` (nanoseconds on the master clock), and records that it was.
    ///
    /// A window not asked about before is always due. After that it is due only on one of the
    /// events that can change komorebi's decision, and at least five seconds after the last time.
    pub fn should_ask(&mut self, hwnd: u32, event: &str, now_ns: u64) -> bool {
        if self.asked.len() >= ASKED_CAPACITY {
            self.asked.clear();
        }

        let due = match self.asked.get(&hwnd) {
            None => true,
            Some(last) => {
                DECIDING_EVENTS.contains(&event) && now_ns.saturating_sub(*last) >= ASK_AGAIN_NS
            }
        };
        if due {
            self.asked.insert(hwnd, now_ns);
        }
        due
    }
}

fn shortened(text: String) -> String {
    if text.chars().count() <= SAYS_LIMIT {
        return text;
    }

    let mut cut: String = text.chars().take(SAYS_LIMIT).collect();
    cut.push('…');
    cut
}

type Inspect = Box<dyn FnMut(u32) -> Option<WindowFacts> + Send>;
type Ask = Box<dyn FnMut(u32) -> Option<String> + Send>;

/// Reads the real state of the window a komorebi event is about.
///
/// A prober is built with two functions, one that reads a window ([`WindowFacts`]) and one that
/// asks komorebi about it, so that the logic (which events are probed, when komorebi is asked) can
/// be tested without Windows or komorebi. [`EventProber::windows`] builds the real one.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::sources::winprobe::{EventProber, WindowFacts};
/// use serde_json::json;
///
/// let mut prober = EventProber::new(
///     |hwnd| Some(WindowFacts {
///         hwnd,
///         at_ns: 0,
///         exists: true,
///         visible: true,
///         cloaked: false,
///         minimized: false,
///         rect: Some([0, 0, 100, 100]),
///         komorebi_says: None,
///     }),
///     |_hwnd| Some("would manage: yes".to_string()),
/// );
///
/// let window = json!({"hwnd": 131072, "title": "t", "exe": "a.exe", "class": "C",
///                     "rect": {"left": 0, "top": 0, "right": 1, "bottom": 1}});
/// let event = json!({"type": "FocusChange", "content": ["SystemForeground", window]});
///
/// let facts = prober.probe(&event, 5_000).ok_or("not probed")?;
/// assert_eq!((facts.hwnd, facts.at_ns), (131_072, 5_000), "stamped when it was read");
/// assert!(facts.shown());
/// assert_eq!(facts.komorebi_says.as_deref(), Some("would manage: yes"), "the first time it is seen");
///
/// // a command, even one that carries a number, is not about a window
/// assert_eq!(prober.probe(&json!({"type": "DebugWindow", "content": 131072}), 6_000), None);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct EventProber {
    inspect: Inspect,
    ask: Ask,
    policy: AskPolicy,
}

impl EventProber {
    /// Returns a prober that reads windows with `inspect` and asks komorebi with `ask`.
    ///
    /// Both are given a window handle. `inspect` returns `None` for a window it cannot read, and
    /// `ask` returns komorebi's answer, or `None` for none.
    pub fn new(
        inspect: impl FnMut(u32) -> Option<WindowFacts> + Send + 'static,
        ask: impl FnMut(u32) -> Option<String> + Send + 'static,
    ) -> Self {
        Self {
            inspect: Box::new(inspect),
            ask: Box::new(ask),
            policy: AskPolicy::default(),
        }
    }

    /// Returns the real prober: Win32 for the window ([`crate::platform::window::inspect`]),
    /// komorebi's socket for its verdict (a `DebugWindow` query, see [`crate::komorebi_link`]).
    ///
    /// komorebi's reply is cut to its first 2,000 characters; an empty reply, and a query that
    /// fails, are no answer. On any host but Windows there is nothing to read, so every probe is
    /// `None`.
    ///
    /// This talks to Windows and to komorebi, so it has no doctest and no unit test: the logic it
    /// drives is covered through [`EventProber::new`] by the tests of this file, and the Win32
    /// reads are checked on a real machine (see `docs/dev/windows-checkpoints.md`).
    #[must_use]
    pub fn windows() -> Self {
        Self::new(crate::platform::window::inspect, |hwnd| {
            let reply =
                crate::komorebi_link::send_query(&crate::komorebi_link::Message::DebugWindow(hwnd))
                    .ok()?;
            (!reply.trim().is_empty()).then(|| shortened(reply))
        })
    }

    /// Probes the window `event` (the JSON komorebi sent) is about, if it is about one, and returns
    /// what was read, with `at_ns` set to `now_ns` (the time on the master clock).
    ///
    /// Only a window-manager event is: a query or a command may carry a number, and one of them,
    /// the `DebugWindow` this asks, is itself reported to subscribers, which must not set off
    /// another probe. The result is `None` for any other event, for an event with no window
    /// handle, and when the window cannot be read. komorebi is asked about the window only if it
    /// still exists and the [`AskPolicy`] says it is due.
    pub fn probe(&mut self, event: &Value, now_ns: u64) -> Option<WindowFacts> {
        let info = Flavor::Windows.rules().describe_event(event);
        if info.family != Family::WindowManager {
            return None;
        }

        let hwnd = info.window_id?;
        let mut facts = (self.inspect)(hwnd)?;
        facts.at_ns = now_ns;
        if facts.exists && self.policy.should_ask(hwnd, &info.name, now_ns) {
            facts.komorebi_says = (self.ask)(hwnd);
        }

        Some(facts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    fn facts(hwnd: u32) -> WindowFacts {
        WindowFacts {
            hwnd,
            at_ns: 0,
            exists: true,
            visible: true,
            cloaked: false,
            minimized: false,
            rect: Some([0, 0, 100, 100]),
            komorebi_says: None,
        }
    }

    fn window(hwnd: i64) -> Value {
        json!({"hwnd": hwnd, "title": "t", "exe": "a.exe", "class": "C",
               "rect": {"left": 0, "top": 0, "right": 1, "bottom": 1}})
    }

    fn event(name: &str, hwnd: i64) -> Value {
        json!({"type": name, "content": ["ObjectShow", window(hwnd)]})
    }

    struct Counting {
        inspected: Arc<AtomicUsize>,
        asked: Arc<AtomicUsize>,
        prober: EventProber,
    }

    fn counting() -> Counting {
        let (inspected, asked) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (i, a) = (inspected.clone(), asked.clone());
        Counting {
            inspected,
            asked,
            prober: EventProber::new(
                move |hwnd| {
                    i.fetch_add(1, Ordering::Relaxed);
                    Some(facts(hwnd))
                },
                move |_| {
                    a.fetch_add(1, Ordering::Relaxed);
                    Some("would manage: yes".to_string())
                },
            ),
        }
    }

    #[test]
    fn a_window_event_is_followed_by_a_read_of_that_window_stamped_when_it_was_read() {
        let mut probe = counting();
        let found = probe
            .prober
            .probe(&event("FocusChange", 131_072), 5_000)
            .expect("facts");
        assert_eq!((found.hwnd, found.at_ns), (131_072, 5_000));
        assert!(found.shown());
        assert_eq!(probe.inspected.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn what_is_not_a_window_event_is_never_probed() {
        let mut probe = counting();
        // a command with a number (the shape of the DebugWindow this probe itself sends), a
        // notification about the machine, a state query, and nonsense
        for not_a_window in [
            json!({"type": "DebugWindow", "content": 131_072}),
            json!({"type": "FocusWorkspaceNumber", "content": 2}),
            json!({"type": "SessionLocked"}),
            json!({"type": "State"}),
            json!("LeftAssociatedVirtualDesktop"),
            json!(12),
        ] {
            assert_eq!(probe.prober.probe(&not_a_window, 1), None, "{not_a_window}");
        }
        assert_eq!(probe.inspected.load(Ordering::Relaxed), 0);
        assert_eq!(probe.asked.load(Ordering::Relaxed), 0, "no feedback loop");
    }

    #[test]
    fn komorebi_is_asked_about_a_window_the_first_time_and_then_after_events_that_can_change_its_mind()
     {
        let mut probe = counting();
        let asked = |probe: &Counting| probe.asked.load(Ordering::Relaxed);

        let first = probe
            .prober
            .probe(&event("FocusChange", 7), 1)
            .expect("facts");
        assert_eq!(first.komorebi_says.as_deref(), Some("would manage: yes"));
        assert_eq!(asked(&probe), 1);

        // focus moving to it again says nothing new, however long it has been
        probe.prober.probe(&event("FocusChange", 7), 60_000_000_000);
        assert_eq!(asked(&probe), 1);

        // a Show after the quiet period can: asked again
        probe.prober.probe(&event("Show", 7), 61_000_000_000);
        assert_eq!(asked(&probe), 2);

        // but not in a flurry
        probe.prober.probe(&event("Show", 7), 62_000_000_000);
        assert_eq!(asked(&probe), 2);

        // and another window is its own first time
        probe.prober.probe(&event("FocusChange", 8), 62_000_000_000);
        assert_eq!(asked(&probe), 3);
    }

    #[test]
    fn a_destroyed_window_is_recorded_as_gone_and_komorebi_is_not_asked_about_it() {
        let mut probe = EventProber::new(
            |hwnd| {
                Some(WindowFacts {
                    exists: false,
                    visible: false,
                    rect: None,
                    ..facts(hwnd)
                })
            },
            |_| panic!("a destroyed window is not asked about"),
        );
        let found = probe.probe(&event("Destroy", 9), 1).expect("facts");
        assert_eq!(found.hidden_how(), Some("destroyed"));
    }

    #[test]
    fn how_a_window_is_hidden_is_named_and_a_shown_one_is_not() {
        let mut window = facts(1);
        assert!(window.shown());
        assert_eq!(window.hidden_how(), None);

        window.cloaked = true;
        assert!(!window.shown());
        assert_eq!(window.hidden_how(), Some("cloaked"));

        window.cloaked = false;
        window.minimized = true;
        assert_eq!(window.hidden_how(), Some("minimized"));

        window.minimized = false;
        window.visible = false;
        assert_eq!(window.hidden_how(), Some("not visible"));
    }

    #[test]
    fn the_facts_roundtrip_and_old_rows_without_them_still_parse() {
        let mut with = facts(5);
        with.komorebi_says = Some("rules matched".to_string());
        let text = serde_json::to_string(&with).expect("json");
        assert_eq!(
            serde_json::from_str::<WindowFacts>(&text).expect("parse"),
            with
        );

        let without = serde_json::to_string(&facts(5)).expect("json");
        assert!(!without.contains("komorebi_says"));
    }

    #[test]
    fn a_long_answer_is_cut_short() {
        let cut = shortened("x".repeat(SAYS_LIMIT * 2));
        assert_eq!(cut.chars().count(), SAYS_LIMIT + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(shortened("short".to_string()), "short");
    }
}
