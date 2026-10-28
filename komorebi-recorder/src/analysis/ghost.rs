//! Ghost tiles: tiles of the layout that hold a window a person cannot see.
//!
//! komorebi gives a tile to every window that its rules do not ignore, and some windows are there
//! for the application's sake and not to be looked at: a helper, an invisible host, a window the
//! application keeps hidden. komorebi tiles it all the same, and the layout then has a hole where a
//! person expects a window. komorebi-application-specific-configuration answers that with an
//! `ignore` rule, which [`crate::asc`] writes from what this module found.
//!
//! Which window a rule is about is not always the whole application: an application that has a
//! ghost window usually has real ones too. Each [`GhostTile`] therefore carries the other windows
//! of the same program that the recording saw ([`GhostTile::siblings`]), so that the rule can be
//! narrow enough to leave them alone.
//!
//! # What is called a ghost tile
//!
//! The recording has to show that komorebi has the window in front of a tile on a workspace that is
//! showing, so that it should be on screen, and that it never was, on at least two reads over half
//! a second, or still not at the last. How it shows that depends on the host, because only one of
//! them can be asked:
//!
//! - **komorebi for Windows.** Windows can be asked whether a window is on screen, and a recording
//!   made with window probes (on by default on Windows) has the answer for the window of each event
//!   ([`crate::sources::winprobe`]). A ghost is a window Windows said was `cloaked` or `not
//!   visible` on every probe of it, and never on screen or minimized (a window that was ever shown
//!   and is hidden now is in a transition, and a minimized one is something a person did).
//! - **komorebi for Mac.** The recorder has no way to ask macOS, but komorebi's state has every
//!   window's frame, read live from the accessibility API. A ghost is a window whose frame was
//!   `zero-sized` or `outside every display` in every state that had it in front on a showing
//!   workspace, and never a frame a person could see. That is weaker evidence than Windows saying
//!   so: it is a window with no visible frame, not a window macOS says is not drawn.
//!
//! A recording without probes, made on Windows, has no ghost tiles here. That is not the same as
//! having none: it is not knowing.
//!
//! A read is taken for the window of each event komorebi sends (on Windows) or from the state that
//! comes with each event (on a Mac), so a ghost tile that komorebi never sent a second event about
//! is not seen: the recording has to have caught komorebi dealing with it twice.
//!
//! # Example
//!
//! Windows says twice, a second apart, that the front window of a showing workspace is cloaked
//! and never that it is on screen:
//!
//! ```rust
//! use komorebi_recorder::analysis::Analysis;
//! use komorebi_recorder::analysis::flavor::Flavor;
//! use komorebi_recorder::sources::winprobe::WindowFacts;
//! use komorebi_recorder::testkit::SessionBuilder;
//! use komorebi_recorder::testkit::windows_state;
//! use komorebi_recorder::testkit::windows_window;
//! use serde_json::json;
//!
//! const GHOST: i64 = 131_072;
//!
//! let ghost = || windows_window(GHOST, "DDM.exe", Some([0, 0, 960, 1040]));
//! let state = || windows_state(65_537, vec![vec![ghost()]]);
//! let event = json!({"type": "FocusChange", "content": ["SystemForeground", ghost()]});
//! let cloaked = WindowFacts {
//!     hwnd: GHOST as u32,
//!     at_ns: 0,
//!     exists: true,
//!     visible: true,
//!     cloaked: true,
//!     minimized: false,
//!     rect: Some([0, 0, 960, 1040]),
//!     komorebi_says: None,
//! };
//!
//! let (session, _) = SessionBuilder::new("doc-ghost")
//!     .flavor(Flavor::Windows)
//!     .event(2.0, event.clone(), Some(state()))
//!     .probe(cloaked.clone())
//!     .event(3.0, event, Some(state()))
//!     .probe(cloaked)
//!     .build();
//!
//! let ghosts = Analysis::open(&session.dir)?.ghost_tiles()?;
//! assert_eq!(ghosts.len(), 1);
//! assert_eq!(ghosts[0].exe, "DDM.exe");
//! assert_eq!(ghosts[0].hidden_how, "cloaked");
//! assert!(ghosts[0].persisted_to_end);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::Analysis;
use super::flavor::Flavor;
use super::invariants;
use super::state::Snapshot;
use crate::asc::Platform;
use crate::sources::winprobe::WindowFacts;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;

/// Reads of one window that said it was not on screen: fewer than this is a glimpse, not a ghost.
const MIN_READS: usize = 2;

/// Reads that said so, over at least this long (unless the last one still did), are not a
/// transition in progress.
const MIN_SPAN_NS: u64 = 500_000_000;

/// komorebi for Windows writes this much of its error text in place of a title, program or class
/// it could not read, and none of it names anything.
const UNREADABLE: &str = "could not get window";

/// Another window of the same program as a ghost tile, which a rule for the ghost must not catch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Sibling {
    /// The window's id in komorebi's state.
    pub id: u64,
    /// Its class. Empty on a Mac, which has none, and for a window komorebi could not read.
    pub class: String,
    /// Its title, which is empty if it has none.
    pub title: String,
}

/// A window komorebi tiles and a person cannot see.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct GhostTile {
    /// Which komorebi it was seen on, and so which rule file it belongs to.
    pub platform: Platform,
    /// The window's id in komorebi's state: its handle on Windows.
    pub hwnd: u64,
    /// The program: an executable file name on Windows (`DDM.exe`), an application name on a Mac.
    /// Empty if komorebi could not read it.
    pub exe: String,
    /// The window class, such as `Chrome_WidgetWin_1`. Empty on a Mac, which has none, and if
    /// komorebi could not read it.
    pub class: String,
    /// The title, which is often empty for a ghost.
    pub title: String,
    /// How the window is not on screen: `cloaked` or `not visible` on Windows, `zero-sized` or
    /// `outside every display` on a Mac.
    pub hidden_how: String,
    /// Where komorebi has the window, in words (`monitor 65537 workspace one (container 0)`).
    pub place: String,
    /// How many reads of the window said so: its probes on Windows, the states that came with the
    /// events on a Mac.
    pub reads: usize,
    /// The sequence number of the first event whose read said so, for `trace`.
    pub first_seq: u64,
    /// When the first such read was taken, in seconds of video time.
    pub first_video_s: f64,
    /// When the last was, in seconds of video time.
    pub last_video_s: f64,
    /// Whether the last read of the window still said so: it was hidden when the recording ended
    /// or when komorebi last looked at it.
    pub persisted_to_end: bool,
    /// The other windows of the same program that komorebi tiled in the recording and that are not
    /// ghost tiles themselves: what the program has besides this.
    pub siblings: Vec<Sibling>,
}

impl GhostTile {
    /// Returns `true` if and only if the ghost tile was there at some time between `start_video_s`
    /// and `end_video_s`, give or take a second: a bug the user marked while a tile was empty is
    /// about it, and one marked well before or after it is not.
    ///
    /// A ghost the recording last saw hidden counts as there until the recording ended, since no
    /// later read says it went away.
    #[must_use]
    pub fn overlaps(&self, start_video_s: f64, end_video_s: f64, session_end_video_s: f64) -> bool {
        const SLACK_S: f64 = 1.0;
        let until = if self.persisted_to_end {
            session_end_video_s.max(self.last_video_s)
        } else {
            self.last_video_s
        };
        self.first_video_s <= end_video_s + SLACK_S && until >= start_video_s - SLACK_S
    }
}

/// What the reads of one window said.
#[derive(Default)]
struct Sightings {
    exe: String,
    class: String,
    title: String,
    place: String,
    hidden_how: &'static str,
    first_seq: u64,
    first_ns: u64,
    last_ns: u64,
    hidden: usize,
    /// It was on screen, or minimized, on some read: not a ghost.
    disqualified: bool,
    /// The most recent read said it was not on screen.
    hidden_last: bool,
}

/// Returns `text` if it names something: komorebi's error text for a title, program or class it
/// could not read does not.
fn readable(text: &str) -> &str {
    if text.starts_with(UNREADABLE) {
        ""
    } else {
        text
    }
}

/// How the probe says a window is hidden, when that is how a ghost is: not destroyed (it is gone)
/// and not minimized (a person did that).
fn ghostly(facts: &WindowFacts) -> Option<&'static str> {
    match facts.hidden_how() {
        Some(how @ ("cloaked" | "not visible")) => Some(how),
        _ => None,
    }
}

/// How a frame that was read live is not one a person can see, if it is not: it has no area to
/// speak of, or no display has any of it.
fn unseeable(snapshot: &Snapshot, frame: &super::state::Rect) -> Option<&'static str> {
    if frame.width <= 1 || frame.height <= 1 {
        Some("zero-sized")
    } else if !snapshot
        .monitors
        .iter()
        .any(|monitor| monitor.size.overlap_area(frame) > 0)
    {
        Some("outside every display")
    } else {
        None
    }
}

impl Analysis {
    /// Returns the ghost tiles of the recording, the longest-lived first.
    ///
    /// What is one, and what a recording needs for there to be any, is in the [module](self)
    /// documentation. A recording of komorebi for Windows made with `--no-probe-windows` has none.
    ///
    /// # Errors
    ///
    /// Returns an error when a state of the recording cannot be read.
    pub fn ghost_tiles(&self) -> eyre::Result<Vec<GhostTile>> {
        let (platform, seen) = match self.flavor() {
            Flavor::Windows => (Platform::Windows, self.sightings_by_probe()?),
            Flavor::Macos => (Platform::Macos, self.sightings_by_frame()?),
        };

        let mut ghosts: Vec<(u64, u64, Sightings)> = seen
            .into_iter()
            .filter(|(_, sightings)| {
                !sightings.disqualified
                    && sightings.hidden >= MIN_READS
                    && (sightings.hidden_last
                        || sightings.last_ns.saturating_sub(sightings.first_ns) >= MIN_SPAN_NS)
            })
            .map(|(id, sightings)| {
                (
                    sightings.last_ns.saturating_sub(sightings.first_ns),
                    id,
                    sightings,
                )
            })
            .collect();
        ghosts.sort_by(|(a, left, _), (b, right, _)| b.cmp(a).then(left.cmp(right)));

        let known = self.windows_seen()?;
        let ghost_ids: HashSet<u64> = ghosts.iter().map(|(_, id, _)| *id).collect();
        Ok(ghosts
            .into_iter()
            .map(|(_, id, sightings)| {
                let siblings = known
                    .iter()
                    .filter(|(other, window)| {
                        **other != id
                            && !ghost_ids.contains(other)
                            && same_program(platform, &window.exe, &sightings.exe)
                    })
                    .map(|(other, window)| Sibling {
                        id: *other,
                        class: readable(&window.class).to_string(),
                        title: readable(&window.title).to_string(),
                    })
                    .collect::<Vec<_>>();
                let mut siblings = siblings;
                siblings.sort_by_key(|sibling| sibling.id);
                GhostTile {
                    platform,
                    hwnd: id,
                    exe: sightings.exe,
                    class: sightings.class,
                    title: sightings.title,
                    hidden_how: sightings.hidden_how.to_string(),
                    place: sightings.place,
                    reads: sightings.hidden,
                    first_seq: sightings.first_seq,
                    first_video_s: self.video_s(sightings.first_ns),
                    last_video_s: self.video_s(sightings.last_ns),
                    persisted_to_end: sightings.hidden_last,
                    siblings,
                }
            })
            .collect())
    }

    /// Every window komorebi had a tile for at some point in the recording, by id.
    fn windows_seen(&self) -> eyre::Result<HashMap<u64, super::state::Window>> {
        let mut known = HashMap::new();
        let mut done = HashSet::new();
        for event in self.events() {
            let Some(state_id) = event.state_id else {
                continue;
            };
            if !done.insert(state_id) {
                continue;
            }
            for (_, window) in self.snapshot(state_id)?.windows() {
                known.entry(window.id).or_insert_with(|| window.clone());
            }
        }
        Ok(known)
    }

    /// What Windows said about each window komorebi sent an event about.
    fn sightings_by_probe(&self) -> eyre::Result<HashMap<u64, Sightings>> {
        let mut seen: HashMap<u64, Sightings> = HashMap::new();

        for event in self.events() {
            let (Some(facts), Some(state_id)) = (&event.probe, event.state_id) else {
                continue;
            };
            if facts.at_ns.saturating_sub(event.recv_mach_ns) > super::probed::STALE_NS {
                continue;
            }
            // komorebi does not manage this program's own windows, by design
            let exe = event.info.exe.as_deref();
            if exe.is_some_and(|exe| exe.to_ascii_lowercase().starts_with("komorebi-recorder")) {
                continue;
            }

            let id = u64::from(facts.hwnd);
            let sightings = seen.entry(id).or_default();
            if facts.shown() || facts.minimized {
                sightings.disqualified = true;
            }
            sightings.hidden_last = false;

            let snapshot = self.snapshot(state_id)?;
            let hidden = invariants::check_probed(&snapshot, facts, exe)
                .into_iter()
                .any(|violation| violation.kind == "visible-window-hidden");
            let Some(how) = ghostly(facts).filter(|_| hidden) else {
                continue;
            };
            let Some((location, window)) = snapshot
                .windows()
                .into_iter()
                .find(|(_, window)| window.id == id)
            else {
                continue;
            };
            sightings.note(
                how,
                event.seq,
                event.recv_mach_ns,
                window,
                &location.describe(),
            );
        }
        Ok(seen)
    }

    /// What the frame komorebi read of each window was, with each event, while the state had it in
    /// front of a tile on a workspace that is showing.
    fn sightings_by_frame(&self) -> eyre::Result<HashMap<u64, Sightings>> {
        let mut seen: HashMap<u64, Sightings> = HashMap::new();

        for event in self.events() {
            let Some(state_id) = event.state_id else {
                continue;
            };
            // each event is a read: a state that did not change is stored once and comes with
            // every event until it does, and that is komorebi still having the same state

            let snapshot = self.snapshot(state_id)?;
            if snapshot.paused {
                continue;
            }
            for (location, window) in snapshot.windows() {
                let sightings = seen.entry(window.id).or_default();
                if invariants::should_be_shown(&snapshot, &location) != Some(true) {
                    // not meant to be on screen now: neither for nor against
                    sightings.hidden_last = false;
                    continue;
                }
                let Some(frame) = &window.rect else {
                    sightings.hidden_last = false;
                    continue;
                };
                let Some(how) = unseeable(&snapshot, frame) else {
                    // a frame a person can see: it is not a ghost
                    sightings.disqualified = true;
                    sightings.hidden_last = false;
                    continue;
                };
                sightings.note(
                    how,
                    event.seq,
                    event.recv_mach_ns,
                    window,
                    &location.describe(),
                );
            }
        }
        Ok(seen)
    }
}

impl Sightings {
    /// Takes in one read that said the window was not on screen.
    fn note(
        &mut self,
        how: &'static str,
        seq: u64,
        at_ns: u64,
        window: &super::state::Window,
        place: &str,
    ) {
        if self.hidden == 0 {
            self.first_seq = seq;
            self.first_ns = at_ns;
            self.hidden_how = how;
            self.exe = readable(&window.exe).to_string();
            self.class = readable(&window.class).to_string();
            self.title = readable(&window.title).to_string();
            self.place = place.to_string();
        }
        self.last_ns = at_ns;
        self.hidden += 1;
        self.hidden_last = true;
    }
}

/// Whether two program names are the same program: Windows ignores case in a file name.
fn same_program(platform: Platform, a: &str, b: &str) -> bool {
    match platform {
        Platform::Windows => a.eq_ignore_ascii_case(b),
        Platform::Macos => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::SessionBuilder;
    use crate::testkit::windows_state;
    use crate::testkit::windows_window;
    use serde_json::json;

    const GHOST: i64 = 131_072;
    const REAL: i64 = 262_144;

    fn facts(hwnd: i64, cloaked: bool, visible: bool, minimized: bool) -> WindowFacts {
        WindowFacts {
            hwnd: u32::try_from(hwnd).expect("hwnd"),
            at_ns: 0,
            exists: true,
            visible,
            cloaked,
            minimized,
            rect: Some([0, 0, 960, 1040]),
            komorebi_says: None,
        }
    }

    fn event(hwnd: i64) -> serde_json::Value {
        json!({"type": "FocusChange", "content": ["SystemForeground",
            windows_window(hwnd, "DDM.exe", Some([0, 0, 960, 1040]))]})
    }

    /// The ghost in front of its tile, with a window of the same program beside it.
    fn state() -> serde_json::Value {
        windows_state(
            65_537,
            vec![
                vec![windows_window(GHOST, "DDM.exe", Some([0, 0, 960, 1040]))],
                vec![windows_window(REAL, "ddm.exe", Some([960, 0, 960, 1040]))],
            ],
        )
    }

    /// The ghost tiles of a recording with the given probes of the ghost, after each of the given
    /// seconds.
    fn ghosts(name: &str, probes: &[(f64, WindowFacts)]) -> Vec<GhostTile> {
        let mut builder = SessionBuilder::new(name).flavor(Flavor::Windows);
        for (at, facts) in probes {
            builder = builder
                .event(*at, event(GHOST), Some(state()))
                .probe(facts.clone());
        }
        let (session, _) = builder.build();
        Analysis::open(&session.dir)
            .expect("open")
            .ghost_tiles()
            .expect("ghost tiles")
    }

    #[test]
    fn a_window_that_was_never_on_screen_is_a_ghost_with_its_identity() {
        let found = ghosts(
            "ghost-found",
            &[
                (2.0, facts(GHOST, true, true, false)),
                (3.0, facts(GHOST, true, true, false)),
            ],
        );

        assert_eq!(found.len(), 1, "{found:#?}");
        let ghost = &found[0];
        assert_eq!(ghost.platform, Platform::Windows);
        assert_eq!(ghost.exe, "DDM.exe");
        assert_eq!(ghost.class, "Window");
        assert_eq!(ghost.hidden_how, "cloaked");
        assert_eq!(ghost.reads, 2);
        assert!(ghost.persisted_to_end);
        assert!(ghost.place.contains("container 0"), "{}", ghost.place);
    }

    #[test]
    fn what_else_the_program_has_is_kept_with_the_ghost() {
        let found = ghosts(
            "ghost-siblings",
            &[
                (2.0, facts(GHOST, true, true, false)),
                (3.0, facts(GHOST, true, true, false)),
            ],
        );

        // the other window is `ddm.exe`: the same program, as Windows sees file names
        assert_eq!(
            found[0].siblings,
            [Sibling {
                id: REAL as u64,
                class: "Window".to_string(),
                title: format!("window {REAL}"),
            }]
        );
    }

    #[test]
    fn a_window_that_is_not_visible_is_a_ghost_too() {
        let found = ghosts(
            "ghost-invisible",
            &[
                (2.0, facts(GHOST, false, false, false)),
                (3.0, facts(GHOST, false, false, false)),
            ],
        );

        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].hidden_how, "not visible");
    }

    #[test]
    fn a_window_that_was_on_screen_at_any_time_is_not() {
        let found = ghosts(
            "ghost-was-shown",
            &[
                (2.0, facts(GHOST, false, true, false)),
                (3.0, facts(GHOST, true, true, false)),
                (4.0, facts(GHOST, true, true, false)),
            ],
        );

        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn a_window_a_person_minimized_is_not() {
        let found = ghosts(
            "ghost-minimized",
            &[
                (2.0, facts(GHOST, false, true, true)),
                (3.0, facts(GHOST, false, true, true)),
            ],
        );

        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn one_probe_is_a_glimpse_and_not_a_ghost() {
        let found = ghosts("ghost-glimpse", &[(2.0, facts(GHOST, true, true, false))]);

        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn a_windows_recording_without_probes_has_none() {
        let (session, _) = SessionBuilder::new("ghost-unprobed")
            .flavor(Flavor::Windows)
            .event(2.0, event(GHOST), Some(state()))
            .event(3.0, event(GHOST), Some(state()))
            .build();

        let found = Analysis::open(&session.dir)
            .expect("open")
            .ghost_tiles()
            .expect("ghost tiles");

        assert!(found.is_empty(), "{found:#?}");
    }

    /// A Mac state: two windows of Safari, one of them in each of two tiles, with the frames and
    /// titles given.
    fn mac_state(ghost_frame: [i64; 4]) -> serde_json::Value {
        use crate::testkit::container;
        use crate::testkit::monitor;
        use crate::testkit::state;
        use crate::testkit::window;
        use crate::testkit::workspace;

        let mut ghost = window(7, "Safari", Some(ghost_frame));
        ghost["details"]["title"] = json!("Reader Helper");
        let mut real = window(8, "Safari", Some([500, 0, 500, 800]));
        real["details"]["title"] = json!("Apple");
        state(
            vec![monitor(
                1,
                [0, 0, 1000, 800],
                vec![workspace(
                    "one",
                    vec![container("c7", vec![ghost]), container("c8", vec![real])],
                    0,
                    &[[0, 0, 500, 800], [500, 0, 500, 800]],
                )],
                0,
            )],
            0,
        )
    }

    fn mac_ghosts(name: &str, frames: &[(f64, [i64; 4])]) -> Vec<GhostTile> {
        let mut builder = SessionBuilder::new(name);
        for (at, frame) in frames {
            builder = builder.event(*at, json!({"type": "Retile"}), Some(mac_state(*frame)));
        }
        let (session, _) = builder.build();
        Analysis::open(&session.dir)
            .expect("open")
            .ghost_tiles()
            .expect("ghost tiles")
    }

    #[test]
    fn on_a_mac_a_tile_whose_frame_is_never_one_a_person_can_see_is_a_ghost() {
        let found = mac_ghosts("ghost-mac", &[(2.0, [0, 0, 0, 0]), (3.0, [0, 0, 0, 0])]);
        assert_eq!(found.len(), 1, "{found:#?}");
        let ghost = &found[0];
        assert_eq!(ghost.platform, Platform::Macos);
        assert_eq!(
            (ghost.exe.as_str(), ghost.title.as_str()),
            ("Safari", "Reader Helper")
        );
        assert_eq!(ghost.class, "");
        assert_eq!(ghost.hidden_how, "zero-sized");
        assert_eq!(ghost.siblings.len(), 1);
        assert_eq!(ghost.siblings[0].title, "Apple");
    }

    #[test]
    fn on_a_mac_a_frame_outside_every_display_is_one_too() {
        let found = mac_ghosts(
            "ghost-mac-away",
            &[(2.0, [9000, 9000, 500, 800]), (3.0, [9000, 9000, 500, 800])],
        );

        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].hidden_how, "outside every display");
    }

    #[test]
    fn on_a_mac_a_frame_that_was_ever_where_it_could_be_seen_is_not_a_ghost() {
        let found = mac_ghosts(
            "ghost-mac-seen",
            &[
                (2.0, [0, 0, 500, 800]),
                (3.0, [0, 0, 0, 0]),
                (4.0, [0, 0, 0, 0]),
            ],
        );

        assert!(found.is_empty(), "{found:#?}");
    }

    /// A Mac with two displays, the second to the left of the main one (so at negative
    /// coordinates, as a Mac puts a display arranged that way), and one window in a tile on the
    /// second with the frame given. `showing` is whether the workspace that has it is the one the
    /// display shows; `paused` is komorebi's pause.
    fn two_displays(frame: [i64; 4], showing: bool, paused: bool) -> serde_json::Value {
        use crate::testkit::container;
        use crate::testkit::monitor;
        use crate::testkit::state;
        use crate::testkit::window;
        use crate::testkit::workspace;

        let main = monitor(
            1,
            [0, 0, 1000, 800],
            vec![workspace("main", Vec::new(), 0, &[])],
            0,
        );
        let left = monitor(
            2,
            [-1000, 0, 1000, 800],
            vec![
                workspace(
                    "away",
                    vec![container("c7", vec![window(7, "Safari", Some(frame))])],
                    0,
                    &[[-1000, 0, 1000, 800]],
                ),
                workspace("here", Vec::new(), 0, &[]),
            ],
            usize::from(!showing),
        );
        let mut value = state(vec![main, left], 0);
        value["is_paused"] = json!(paused);
        value
    }

    fn two_display_ghosts(name: &str, frames: &[[i64; 4]], showing: bool, paused: bool) -> usize {
        let mut builder = SessionBuilder::new(name);
        for (index, frame) in frames.iter().enumerate() {
            builder = builder.event(
                2.0 + index as f64,
                json!({"type": "Retile"}),
                Some(two_displays(*frame, showing, paused)),
            );
        }
        let (session, _) = builder.build();
        Analysis::open(&session.dir)
            .expect("open")
            .ghost_tiles()
            .expect("ghost tiles")
            .len()
    }

    #[test]
    fn on_a_mac_a_window_on_a_display_at_negative_coordinates_can_be_seen() {
        // the whole of the left display: nothing about it is outside every display
        let on_it = [-1000, 0, 1000, 800];
        assert_eq!(
            two_display_ghosts("ghost-mac-left", &[on_it, on_it, on_it], true, false),
            0
        );

        // the same shape, beyond the left display's edge, is outside all of them
        let past_it = [-3000, 0, 1000, 800];
        assert_eq!(
            two_display_ghosts("ghost-mac-past", &[past_it, past_it, past_it], true, false),
            1
        );
    }

    #[test]
    fn on_a_mac_one_read_is_not_enough_to_call_a_tile_a_ghost() {
        let nowhere = [0, 0, 0, 0];
        assert_eq!(
            two_display_ghosts("ghost-mac-once", &[nowhere], true, false),
            0
        );
        assert_eq!(
            two_display_ghosts("ghost-mac-twice", &[nowhere, nowhere], true, false),
            1
        );
    }

    #[test]
    fn on_a_mac_a_window_of_a_workspace_that_is_not_showing_is_not_a_ghost() {
        let nowhere = [0, 0, 0, 0];
        assert_eq!(
            two_display_ghosts("ghost-mac-hidden-workspace", &[nowhere; 3], false, false),
            0
        );
    }

    #[test]
    fn on_a_mac_nothing_is_a_ghost_while_komorebi_is_paused() {
        let nowhere = [0, 0, 0, 0];
        assert_eq!(
            two_display_ghosts("ghost-mac-paused", &[nowhere; 3], true, true),
            0
        );
    }

    #[test]
    fn a_ghost_is_about_a_bug_that_was_marked_while_it_was_there() {
        let ghost = GhostTile {
            platform: Platform::Windows,
            hwnd: 1,
            exe: "DDM.exe".into(),
            class: "Window".into(),
            title: String::new(),
            hidden_how: "cloaked".into(),
            place: String::new(),
            reads: 2,
            first_seq: 1,
            first_video_s: 10.0,
            last_video_s: 12.0,
            persisted_to_end: false,
            siblings: Vec::new(),
        };

        assert!(ghost.overlaps(11.0, 13.0, 60.0));
        assert!(ghost.overlaps(0.0, 9.5, 60.0), "a second of slack");
        assert!(!ghost.overlaps(0.0, 5.0, 60.0));
        assert!(!ghost.overlaps(20.0, 30.0, 60.0));

        let lasting = GhostTile {
            persisted_to_end: true,
            ..ghost
        };
        assert!(lasting.overlaps(20.0, 30.0, 60.0), "still there at the end");
    }

    #[test]
    fn what_komorebi_could_not_read_is_not_an_identity() {
        assert_eq!(readable("could not get window class"), "");
        assert_eq!(readable("Chrome_WidgetWin_1"), "Chrome_WidgetWin_1");
    }
}
