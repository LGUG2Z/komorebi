//! The window: the Record tab, opening a session and keeping everything on screen in step.
//!
//! [`ViewerApp`] is the whole application. It holds the recorder ([`RecorderApp`]), which is the
//! pinned first tab and cannot be closed, and after it a tab for each session, each with a
//! [`Screen`]: nothing opened yet, a session loading, a session that failed to open, or an [`Open`]
//! session. [`ViewerApp::ui`] draws one frame of the tab bar and of the tab that is showing. An
//! [`Open`] is where the state of a session being viewed lives (the
//! playhead's [`Playback`], the visible stretch of the timeline, the selection, which event or
//! finding or bug region is selected, the form being filled in), and its methods are spread
//! over the files that draw each part: this file keeps it in step with the keyboard, and the lanes,
//! the inspector, the video and the composer each add the methods that draw and act on their
//! part.
//!
//! Everything here needs an `egui::Context` to draw, so none of it has an example. The tests at the
//! bottom of this file run [`ViewerApp::ui`] headlessly through `egui::Context::run` with real key
//! events, against sessions made by the test kit, and are what covers the behavior.

use super::ipc;
use super::keymap;
use super::keymap::Action;
use super::loaded::Loaded;
use super::loaded::name_of;
use super::loaded::tab_name;
use super::prefs::Prefs;
use super::theme;
use super::timeline;
use super::timeline::View;
use crate::analysis::guard::SessionPath;
use crate::analysis::state::Snapshot;
use crate::analysis::trace::TraceData;
use crate::app::RecorderApp;
use crate::playback::Playback;
use color_eyre::eyre;
use crossbeam_channel::Receiver;
use eframe::egui;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

/// How wide decoded frames are asked for, in pixels: enough for a large window on a retina
/// display. It is what [`Playback::start`] is given.
pub const DECODE_WIDTH: u32 = 1600;

/// Which tab of the inspector on the right is showing.
///
/// The inspector is described in full in the private `inspector` module of the viewer; in short,
/// each tab answers one question about the playhead or about the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// The state komorebi was in at the playhead, what is wrong in it, and the events just before.
    Now,
    /// One selected event end to end: what its handler logged, what changed and who changed it, and
    /// what the rest of the system did around it.
    Event,
    /// The findings: what looks wrong in the recording, ranked.
    Findings,
    /// The bug regions marked, and the notes and findings annotated, with the form to mark one.
    Bugs,
    /// komorebi's own log, following the playhead.
    Log,
}

/// A frame of a display uploaded to the graphics card, and which frame it is.
///
/// A new [`Frame`](crate::playback::Frame) from [`Playback`] is uploaded again when it differs
/// from the one in the tile, so the same picture is never uploaded twice.
pub struct Tile {
    /// The picture, as a texture on the graphics card.
    pub texture: egui::TextureHandle,
    /// Which [`Frame`](crate::playback::Frame) it was made from, by address: a new frame is a new
    /// `Arc`.
    pub frame: usize,
}

/// A session, open and showing: everything the window keeps for the session being viewed.
///
/// It is made by [`ViewerApp`] when a session has loaded, and it owns the session's data
/// ([`Loaded`]), the [`Playback`] of its video and the state of every part of the window. It also
/// owns the background scan for changes in the picture, which stops when it is dropped.
///
/// A window is needed to draw it (see [`Open::ui`]), and the fields are public so that the tests
/// can look at what a key press did.
pub struct Open {
    /// The session, read.
    pub loaded: Loaded,
    /// The playhead and the decoding of the video.
    pub playback: Playback,
    /// The picture of each display last uploaded, by display id.
    pub tiles: HashMap<u32, Tile>,
    /// The display that stepping acts on and whose picture is outlined. It is picked by clicking
    /// a picture or with the number keys, and is the first display with video when a session opens.
    /// `None` if no display has video.
    pub focus: Option<u32>,
    /// The stretch of the recording the timeline shows.
    pub view: View,
    /// A range picked on the timeline, as `(start, end)` in session video seconds: marked with `[`
    /// and `]` or by dragging with Shift.
    pub selection: Option<(f64, f64)>,
    /// The sequence number of the event selected, whose trace the Event tab shows.
    pub selected_event: Option<u64>,
    /// The index into [`Loaded::findings`] of the finding selected.
    pub selected_finding: Option<usize>,
    /// The id of the annotation selected in the Bugs tab and on the bugs lane.
    pub selected_bug: Option<String>,
    /// The tab of the inspector that is showing.
    pub tab: Tab,
    /// Whether komorebi's state is drawn over the video (the "state overlay" box and `O`). It is
    /// kept in the preferences.
    pub overlay: bool,
    /// Whether the list of keys is open.
    pub help: bool,
    /// The form for marking or editing a bug region, while it is open.
    pub composer: Option<super::composer::Composer>,
    /// The form for proposing a rule for a ghost tile, while it is open.
    pub rule: Option<super::rules::RuleForm>,
    /// The trace of each event looked at, by sequence number, worked out once.
    pub traces: HashMap<u64, Arc<TraceData>>,
    /// The OS trace around each event looked at, by sequence number, worked out once.
    pub os_traces: HashMap<u64, Arc<crate::analysis::os_trace::OsTraceData>>,
    /// The state the overlay draws: the id of the state notification it came from and the state.
    /// It is kept while the playhead stays within the same notification (see
    /// [`Open::state_at`]).
    pub state: Option<(u64, Arc<Snapshot>)>,
    /// A note to show in the top bar and when it was said. The bar shows it for four seconds.
    pub message: Option<(String, Instant)>,
    /// Annotations deleted, newest last, to bring back.
    pub deleted: Vec<crate::annotations::AnnotationView>,
    /// The drag going on across the lanes of the timeline, if one is.
    pub drag: Option<super::lanes::Drag>,
    /// When the picture changed, found in the background after opening. `None` until the scan
    /// finishes, and after it fails.
    pub changes: Option<crate::analysis::changes::ChangesData>,
    /// Where the scan sends its result, until it arrives. `None` once it has, and when there is
    /// nothing to scan (no display has video).
    pub changes_result: Option<Receiver<eyre::Result<crate::analysis::changes::ChangesData>>>,
    /// Set to stop the background scan when this session is closed.
    changes_cancel: Arc<std::sync::atomic::AtomicBool>,
    /// A bug region whose edge is being dragged: its id and where its start and end would be.
    pub edge_preview: Option<(String, f64, f64)>,
}

/// What one tab of the window is showing: a session, or what stands in for one.
///
/// Every tab of a [`ViewerApp`] is on exactly one of these. [`ViewerApp::open`] puts a `Loading`
/// in the tab it opens a session in, and from there it becomes `Open` or `Failed` when the load
/// finishes.
pub enum Screen {
    /// Nothing opened in this tab: the start screen, with the recent sessions that still exist.
    Empty,
    /// A session is being read in the background: a spinner.
    Loading {
        /// The session being opened.
        dir: PathBuf,
        /// Where the load's result arrives.
        result: Receiver<eyre::Result<Loaded>>,
    },
    /// The session would not open: the start screen again, with the reason.
    Failed {
        /// The session that was being opened.
        dir: PathBuf,
        /// Why it would not open: the error with its causes, on one line.
        message: String,
    },
    /// A session, open and showing.
    Open(Box<Open>),
}

impl Screen {
    /// Returns the session this tab is opening, has failed to open or has open, if it has one.
    #[must_use]
    pub fn dir(&self) -> Option<&Path> {
        match self {
            Self::Empty => None,
            Self::Loading { dir, .. } | Self::Failed { dir, .. } => Some(dir),
            Self::Open(open) => Some(&open.loaded.dir),
        }
    }

    /// Whether a session can be opened into this tab without losing anything: it shows the start
    /// screen, or a failure.
    #[must_use]
    pub fn is_free(&self) -> bool {
        matches!(self, Self::Empty | Self::Failed { .. })
    }
}

/// The application: the recorder, the preferences and the tabs of the window.
///
/// This is what [`run`](super::run) hands to `eframe`. [`ViewerApp::new`] makes one showing the
/// Record tab with no session tabs, [`ViewerApp::open`] opens a session in a tab, and
/// [`ViewerApp::ui`] draws a frame. The Record tab is always first and cannot be closed; the
/// session tabs ([`ViewerApp::tabs`]) follow it, and closing the last of them leaves the Record
/// tab showing. Exactly one tab is showing; the others keep their state (an open session keeps its
/// playhead, its selection and its scan) but their video is paused, while a recording carries on
/// whichever tab is showing. It needs a window to draw and so has no example.
///
/// # The two sizes
///
/// The window is either *compact*, the small always-on-top control window that shows the Record tab
/// alone, or expanded, with the tab bar. Starting with no session to show gives the size the
/// preferences remember ([`Prefs::compact`]); showing any session tab expands it, and the Compact
/// button of the Record tab goes back. The window is resized and pinned on top with viewport
/// commands, by [`ViewerApp::ui`], whenever the mode changed.
///
/// # Opening a session
///
/// Every way of opening one (the buttons, the recent list, the system's folder dialog, a request
/// from another process: [`ViewerApp::handle_request`]) goes through [`ViewerApp::open`], which
/// follows one rule: if a tab already has the session, that tab is shown; otherwise, if the
/// current tab is a start screen or a failure, the session loads into it; otherwise it loads into a
/// new tab, which is shown. A tab is only ever closed by the user.
pub struct ViewerApp {
    /// The viewer's preferences. Saved when a session is opened and when the state overlay is
    /// switched.
    pub prefs: Prefs,
    prefs_dir: PathBuf,
    recorder: RecorderApp,
    /// Whether the Record tab, rather than a session tab, is the one showing.
    record_active: bool,
    tabs: Vec<Screen>,
    /// The session tab showing, or the one that was when the Record tab is.
    active: usize,
    compact: bool,
    /// Whether the window is kept on top while it is not compact.
    pinned: bool,
    /// The (compact, on top) the window was last put in, so it is only told when that changes.
    applied: (bool, bool),
    /// The size of the expanded window, to go back to after being compact.
    full_size: egui::Vec2,
    picker: Option<Receiver<Option<PathBuf>>>,
    server: Option<ipc::Server>,
    release_focus: bool,
}

/// Ask the user for a folder with the system's own dialog, off the UI thread.
fn pick_folder() -> Receiver<Option<PathBuf>> {
    let (sender, receiver) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = sender.send(crate::platform::shell::pick_folder("Choose a recording"));
    });
    receiver
}

/// The path as sessions are told apart by: made absolute, with symbolic links followed, and on
/// Windows without the `\\?\` prefix. A path that cannot be resolved (it does not exist) is kept
/// as given, and then fails to open with the reason.
fn canonical(dir: &Path) -> PathBuf {
    dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())
}

impl ViewerApp {
    /// Returns an app showing the Record tab, with the preferences kept in `prefs_dir`, and no
    /// session tabs.
    ///
    /// The preferences are read with [`Prefs::load`], so a missing or damaged file gives the
    /// defaults; the window is compact if they say so. Nothing is opened: see [`ViewerApp::open`].
    #[must_use]
    pub fn new(prefs_dir: PathBuf) -> Self {
        Self::start(
            prefs_dir,
            super::Launch::record(crate::cli::RecordArgs::default()),
        )
    }

    /// Returns the app a launch asks for: the recorder seeded with its options, and its sessions
    /// opened (see [`ViewerApp::open`]).
    ///
    /// A launch that shows the Record tab is compact if the preferences say so; one that shows
    /// sessions is expanded, and with none shows a start screen. The preferences are not changed
    /// by that: only the Compact and Expand buttons write them.
    #[must_use]
    pub fn start(prefs_dir: PathBuf, launch: super::Launch) -> Self {
        let prefs = Prefs::load(&prefs_dir);
        let compact = launch.starts_compact(&prefs);
        let mut app = Self {
            prefs,
            prefs_dir,
            recorder: RecorderApp::new(launch.record),
            record_active: true,
            tabs: Vec::new(),
            active: 0,
            compact,
            pinned: false,
            applied: (compact, compact),
            full_size: egui::vec2(FULL_SIZE[0], FULL_SIZE[1]),
            picker: None,
            server: None,
            release_focus: false,
        };
        if matches!(launch.show, super::Show::Sessions) {
            for dir in &launch.sessions {
                app.open(dir);
            }
            if launch.sessions.is_empty() {
                app.new_tab();
            }
        }
        app
    }

    /// Returns the recorder, the content of the Record tab.
    #[must_use]
    pub fn recorder(&self) -> &RecorderApp {
        &self.recorder
    }

    /// Returns the recorder, to change.
    pub fn recorder_mut(&mut self) -> &mut RecorderApp {
        &mut self.recorder
    }

    /// Whether the Record tab, rather than a session tab, is showing.
    #[must_use]
    pub fn record_active(&self) -> bool {
        self.record_active
    }

    /// Whether the window is the small control window.
    #[must_use]
    pub fn is_compact(&self) -> bool {
        self.compact
    }

    /// Whether the window is on top: always when compact, and when pinned otherwise.
    #[must_use]
    pub fn is_on_top(&self) -> bool {
        self.compact || self.pinned
    }

    /// Switches between the small control window and the full one, and remembers which in the
    /// preferences. Becoming compact shows the Record tab, which is all a compact window has;
    /// expanding shows the tab bar and no longer keeps the window on top.
    pub fn toggle_compact(&mut self) {
        let compact = !self.compact;
        self.set_compact(compact);
        self.prefs.compact = compact;
        self.prefs.save(&self.prefs_dir);
    }

    /// Makes the window compact or not, without touching the preferences: what opening a session
    /// does (it expands the window so the session shows). The resizing itself is done by the next
    /// [`ViewerApp::ui`].
    pub fn set_compact(&mut self, compact: bool) {
        if compact == self.compact {
            return;
        }
        self.compact = compact;
        if compact {
            self.show_record();
        } else {
            self.pinned = false;
        }
    }

    /// Shows the Record tab. The session tab that was showing is paused.
    pub fn show_record(&mut self) {
        self.record_active = true;
        self.pause_hidden();
    }

    /// Takes requests from other processes through `server` from now on: see
    /// [`ViewerApp::handle_request`]. The socket is removed when the viewer is dropped.
    pub fn listen(&mut self, server: ipc::Server) {
        self.server = Some(server);
    }

    /// Returns every session tab, in order: the ones after the Record tab.
    #[must_use]
    pub fn tabs(&self) -> &[Screen] {
        &self.tabs
    }

    /// Returns the index, among [`ViewerApp::tabs`], of the session tab being shown, or that of the
    /// one that was shown last while the Record tab is.
    #[must_use]
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Returns what the session tab being shown has in it, or `None` while the Record tab is
    /// showing.
    #[must_use]
    pub fn screen(&self) -> Option<&Screen> {
        if self.record_active {
            None
        } else {
            self.tabs.get(self.active)
        }
    }

    /// Returns what the session tab being shown has in it, to change, or `None` while the Record
    /// tab is showing.
    pub fn screen_mut(&mut self) -> Option<&mut Screen> {
        if self.record_active {
            None
        } else {
            self.tabs.get_mut(self.active)
        }
    }

    /// Returns the index of the tab that has the session in `dir` (already [`canonical`]) in it.
    fn find(&self, dir: &Path) -> Option<usize> {
        self.tabs.iter().position(|tab| {
            tab.dir()
                .is_some_and(|have| have == dir || canonical(have) == dir)
        })
    }

    /// Opens the session in `dir` in a tab, in the background: see the [type
    /// documentation](ViewerApp#opening-a-session) for which tab.
    ///
    /// The tab is shown at once. It is [`Screen::Loading`] and [`ViewerApp::ui`] brings the result
    /// in when the load has finished: an [`Open`] session, or [`Screen::Failed`] with the reason. A
    /// session that cannot be opened is therefore not an error here; see [`Loaded::load`] for what
    /// can go wrong. A session that is already open is not loaded again; one that failed is
    /// tried again.
    pub fn open(&mut self, dir: &Path) {
        let dir = canonical(dir);
        let slot = match self.find(&dir) {
            Some(slot) if matches!(self.tabs[slot], Screen::Failed { .. }) => slot,
            Some(slot) => {
                self.focus_tab(slot);
                return;
            }
            None => self.slot_for_new_session(),
        };

        let (sender, result) = crossbeam_channel::bounded(1);
        let target = dir.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Loaded::load(&target));
        });
        self.tabs[slot] = Screen::Loading { dir, result };
        self.focus_tab(slot);
    }

    /// Returns the tab a session that is not open yet goes in: the current one if it is a free
    /// session tab, else a new one at the end. The Record tab is never one.
    fn slot_for_new_session(&mut self) -> usize {
        if !self.record_active && self.tabs.get(self.active).is_some_and(Screen::is_free) {
            self.active
        } else {
            self.tabs.push(Screen::Empty);
            self.tabs.len() - 1
        }
    }

    /// Opens the session in `dir` and waits for it: for tests, which have nothing to draw
    /// meanwhile.
    ///
    /// On success the tab is [`Screen::Open`] (found, or made, as [`ViewerApp::open`] does), the
    /// session is at the top of the recent sessions (and the preferences are saved), and the
    /// playhead is a second before where the first bug region starts, if there is one. A session
    /// that is already open is only shown.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be loaded (see [`Loaded::load`]), if `dir` is not a
    /// session directory, or if the playback cannot be started (see [`Playback::start`]). The
    /// tabs are left as they were: unlike [`ViewerApp::open`], a failure is not shown in the window.
    pub fn open_now(&mut self, dir: &Path) -> eyre::Result<()> {
        let dir = canonical(dir);
        if let Some(slot) = self.find(&dir)
            && matches!(self.tabs[slot], Screen::Open(_))
        {
            self.focus_tab(slot);
            return Ok(());
        }
        let loaded = Loaded::load(&dir)?;
        let open = self.build_open(loaded)?;
        let slot = match self.find(&dir) {
            Some(slot) => slot,
            None => self.slot_for_new_session(),
        };
        self.tabs[slot] = Screen::Open(Box::new(open));
        self.focus_tab(slot);
        Ok(())
    }

    fn build_open(&mut self, loaded: Loaded) -> eyre::Result<Open> {
        let guard = SessionPath::new(&loaded.dir)?;
        let playback = Playback::start(loaded.analysis.manifest.clone(), guard, DECODE_WIDTH)?;
        self.prefs.opened(&loaded.dir);
        self.prefs.save(&self.prefs_dir);

        let first_bug = loaded.bugs().next().map(|bug| bug.id.clone());
        let focus = loaded.displays.first().map(|display| display.id);
        let duration = loaded.duration_s;
        let start = loaded.bugs().next().map(|bug| bug.start_video_s);
        let mut open = Open {
            view: View::whole(duration),
            selection: None,
            selected_event: None,
            selected_finding: None,
            selected_bug: first_bug,
            tab: if loaded.bugs().next().is_some() {
                Tab::Bugs
            } else {
                Tab::Findings
            },
            overlay: self.prefs.overlay,
            help: false,
            composer: None,
            rule: None,
            traces: HashMap::new(),
            os_traces: HashMap::new(),
            state: None,
            message: None,
            deleted: Vec::new(),
            tiles: HashMap::new(),
            focus,
            playback,
            loaded,
            drag: None,
            changes: None,
            changes_result: None,
            changes_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            edge_preview: None,
        };
        open.scan_for_changes();

        // start where the user said the bug begins
        if let Some(start) = start {
            open.playback.seek((start - 1.0).max(0.0));
        }
        Ok(open)
    }

    /// Adds a start-screen session tab at the end and shows it.
    pub fn new_tab(&mut self) {
        self.tabs.push(Screen::Empty);
        self.focus_tab(self.tabs.len() - 1);
    }

    /// Shows session tab `index`, and expands the window if it was compact, so the session shows.
    /// The tab that was showing is paused, so that only the tab in view decodes video. An index
    /// beyond the tabs does nothing.
    pub fn focus_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.active = index;
        self.record_active = false;
        self.set_compact(false);
        self.pause_hidden();
        if let Screen::Open(open) = &mut self.tabs[index] {
            // the overlay is one setting for the window, not one per tab
            open.overlay = self.prefs.overlay;
        }
    }

    /// Pauses every tab but the active one (all of them, with the Record tab showing). Decoding
    /// stops with the playhead, and a hidden tab's scan for changes carries on.
    fn pause_hidden(&mut self) {
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if (index != self.active || self.record_active)
                && let Screen::Open(open) = tab
                && open.playback.is_playing()
            {
                open.playback.pause();
            }
        }
    }

    /// Closes session tab `index`; the Record tab is not one and cannot be closed. Closing a
    /// session stops its scan for changes and its playback; closing the last session tab shows the
    /// Record tab (a start screen is only there if `+` adds it). The tab that takes the closed
    /// one's place is shown if it was the one showing. An index beyond the tabs does nothing.
    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        // dropping an Open cancels its scan and stops its playback
        drop(self.tabs.remove(index));
        if self.tabs.is_empty() {
            self.active = 0;
            self.show_record();
            return;
        }
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        if !self.record_active {
            self.focus_tab(self.active);
        }
    }

    /// Carries out a tab key: see [`Action::NewTab`] and the others for which are tab actions. Any
    /// other action does nothing.
    ///
    /// The Record tab is part of the ring `Next` and `Previous` go round (it comes first), `GoToTab`
    /// and `LastTab` count session tabs only, and `CloseTab` does nothing on the Record tab.
    pub fn apply_tab_action(&mut self, action: Action) {
        let count = self.tabs.len();
        let position = if self.record_active {
            0
        } else {
            self.active + 1
        };
        let total = count + 1;
        match action {
            Action::NewTab => self.new_tab(),
            Action::CloseTab if !self.record_active => self.close_tab(self.active),
            Action::NextTab => self.focus_position((position + 1) % total),
            Action::PreviousTab => self.focus_position((position + total - 1) % total),
            Action::GoToTab(index) => self.focus_tab(index),
            Action::LastTab if count > 0 => self.focus_tab(count - 1),
            Action::LastTab | Action::ShowRecord => self.show_record(),
            _ => {}
        }
    }

    /// Shows the tab at `position` of the whole bar: `0` is the Record tab, then the sessions.
    fn focus_position(&mut self, position: usize) {
        match position.checked_sub(1) {
            None => self.show_record(),
            Some(index) => self.focus_tab(index),
        }
    }

    /// Carries out a request from another process (see [`ipc::Request`]): opens each session as
    /// [`ViewerApp::open`] does, which expands the window if it was compact; for a request with
    /// none, shows the Record tab if it asked for that (`gui` does), else a start-screen tab (a new
    /// one unless the current tab is one already). A request that names the Record tab shows it
    /// after any sessions are opened. Returns whether the window should be raised.
    pub fn handle_request(&mut self, request: &ipc::Request) -> bool {
        let record = request.tab == Some(ipc::TabName::Record);
        if request.open.is_empty() {
            if !record && !matches!(self.screen(), Some(Screen::Empty)) {
                self.new_tab();
            }
        } else {
            for dir in &request.open {
                self.open(dir);
            }
        }
        if record {
            self.show_record();
        }
        request.focus
    }

    /// Takes in the requests that arrived from other processes, and raises the window if one asked.
    fn poll_requests(&mut self, ctx: &egui::Context) {
        let Some(server) = &self.server else {
            return;
        };
        let requests = server.requests();
        let mut raise = false;
        for request in &requests {
            raise |= self.handle_request(request);
        }
        if raise {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// Bring finished loads into their tabs, or show why they failed.
    fn poll_loading(&mut self) {
        for index in 0..self.tabs.len() {
            let Screen::Loading { dir, result } = &self.tabs[index] else {
                continue;
            };
            let dir = dir.clone();
            let outcome = match result.try_recv() {
                Ok(outcome) => outcome,
                Err(crossbeam_channel::TryRecvError::Empty) => continue,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    Err(eyre::eyre!("the thread reading the session stopped"))
                }
            };
            self.tabs[index] = match outcome.and_then(|loaded| self.build_open(loaded)) {
                Ok(open) => Screen::Open(Box::new(open)),
                Err(error) => Screen::Failed {
                    dir,
                    message: format!("{error:#}"),
                },
            };
            // a session that finished loading in a tab out of view starts paused
            self.pause_hidden();
        }
    }

    fn poll_picker(&mut self) {
        let Some(picker) = &self.picker else {
            return;
        };
        let Ok(chosen) = picker.try_recv() else {
            return;
        };

        self.picker = None;
        if let Some(dir) = chosen {
            self.open(&dir);
        }
    }

    /// Reads the tab keys, unless a text box wants the keyboard. The keys of the session in the tab
    /// being shown are read by [`Open::ui`].
    ///
    /// Returns whether `Ctrl` `Tab` was pressed: egui also takes that as a request to move the
    /// keyboard focus to the next widget, which would make the window think a text box wants the
    /// keyboard, so the focus is let go of again at the start of the next frame.
    fn tab_keys(&mut self, ctx: &egui::Context) -> bool {
        if std::mem::take(&mut self.release_focus) {
            // egui moved the focus when the last frame ended
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
        if ctx.wants_keyboard_input() {
            return false;
        }
        let ctrl_tab = ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Tab,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.ctrl
                )
            })
        });
        let actions: Vec<Action> = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => keymap::action(*key, *modifiers, None),
                    _ => None,
                })
                .filter(|action| action.is_tab_action())
                .collect()
        });
        for action in actions {
            self.apply_tab_action(action);
        }
        ctrl_tab
    }

    /// Draws one frame of the window.
    ///
    /// It first takes in requests from other processes, finished loads and a folder chosen in the
    /// system's dialog, then draws the tab bar, the top bar of the tab being shown and its screen,
    /// and saves the preferences if the state overlay was switched. While a session is loading, or
    /// the dialog is open, it asks `ctx` to draw again soon, so the result shows without anyone
    /// touching the window. An open session draws itself with [`Open::ui`].
    ///
    /// Needs an `egui::Context`, so a window is needed (the tests of this module draw frames
    /// headlessly with `egui::Context::run`).
    pub fn ui(&mut self, ctx: &egui::Context) {
        // whichever tab is showing, a recording goes on and is finished
        self.recorder.poll(ctx);
        self.poll_requests(ctx);
        self.poll_loading();
        self.poll_picker();
        let ctrl_tab = self.tab_keys(ctx);
        // a session needs the room of the full window
        if !self.record_active {
            self.set_compact(false);
        }
        self.pause_hidden();
        self.remember_full_size(ctx);

        let mut to_open: Option<PathBuf> = None;
        let mut choose = false;
        let mut bar = TabBar::default();
        let (label, recording) = self.recorder.tab_label();
        let record_tab = RecordTab {
            label,
            recording,
            selected: self.record_active,
        };

        // the compact window is the Record tab alone
        if !self.compact {
            egui::TopBottomPanel::top("top")
                .frame(
                    egui::Frame::new()
                        .fill(theme::BACKGROUND)
                        .inner_margin(egui::Margin::symmetric(10, 6)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        // leave room for the window controls that sit over the content
                        if cfg!(target_os = "macos") {
                            ui.add_space(70.0);
                        }
                        bar = tab_bar(ui, &record_tab, &self.tabs, self.active);
                        if ui.button("Open…").clicked() {
                            choose = true;
                        }
                        if recording {
                            ui.checkbox(&mut self.pinned, "pin on top")
                                .on_hover_text("keep this window above the others while recording");
                        }
                    });
                    if !self.record_active {
                        let active = self.active;
                        match &mut self.tabs[active] {
                            Screen::Open(open) => {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(name_of(&open.loaded.dir)).strong(),
                                    );
                                    open.top_bar(ui, &mut self.prefs);
                                });
                            }
                            Screen::Loading { dir, .. } => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(format!("opening {}…", name_of(dir)));
                                });
                            }
                            _ => {}
                        }
                    }
                });
        }

        let active = self.active;
        if self.record_active {
            let compact = self.compact;
            let mut output = crate::app::RecorderOutput::default();
            egui::CentralPanel::default().show(ctx, |ui| {
                // leave room for the window controls that sit over the content
                if cfg!(target_os = "macos") && compact {
                    ui.add_space(24.0);
                }
                output = self.recorder.ui(ui, compact);
            });
            choose |= output.choose;
            to_open = output.open;
            if output.toggle_compact {
                self.toggle_compact();
            }
        } else {
            match &mut self.tabs[active] {
                Screen::Open(open) => open.ui(ctx),
                Screen::Empty => {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        to_open = start_screen(ui, &self.prefs, None);
                    });
                }
                Screen::Failed { dir, message } => {
                    let (dir, message) = (dir.clone(), message.clone());
                    egui::CentralPanel::default().show(ctx, |ui| {
                        to_open = start_screen(ui, &self.prefs, Some((&dir, &message)));
                    });
                }
                Screen::Loading { .. } => {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.centered_and_justified(|ui| ui.spinner());
                    });
                }
            }
        }

        // a load in any tab, shown or not, is brought in by a later frame
        if self
            .tabs
            .iter()
            .any(|tab| matches!(tab, Screen::Loading { .. }))
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if choose && self.picker.is_none() {
            self.picker = Some(pick_folder());
        }
        if let Some(dir) = to_open {
            self.open(&dir);
        }
        if bar.new_tab {
            self.new_tab();
        }
        if bar.select_record {
            self.show_record();
        }
        if let Some(index) = bar.select {
            self.focus_tab(index);
        }
        if let Some(index) = bar.close {
            self.close_tab(index);
        }
        self.release_focus = ctrl_tab;
        if self.picker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.server.is_some() {
            // requests wake the window themselves; this is for a window that is minimised
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        if !self.record_active
            && let Some(Screen::Open(open)) = self.tabs.get(self.active)
            && open.overlay != self.prefs.overlay
        {
            self.prefs.overlay = open.overlay;
            self.prefs.save(&self.prefs_dir);
        }
        self.sync_window(ctx);
    }

    /// Notes the size of the window while it is the full one, to go back to after being compact.
    fn remember_full_size(&mut self, ctx: &egui::Context) {
        if self.compact {
            return;
        }
        if let Some(rect) = ctx.input(|input| input.viewport().inner_rect) {
            let size = rect.size();
            // not the middle of a resize from the small window
            if size.x >= FULL_MIN[0] && size.y >= FULL_MIN[1] {
                self.full_size = size;
            }
        }
    }

    /// Resizes and pins the window if its mode (compact, on top) is not the one it was last put in.
    fn sync_window(&mut self, ctx: &egui::Context) {
        let want = (self.compact, self.is_on_top());
        if want == self.applied {
            return;
        }
        if want.0 != self.applied.0 {
            let (size, min) = if want.0 {
                (egui::vec2(COMPACT_SIZE[0], COMPACT_SIZE[1]), COMPACT_MIN)
            } else {
                (self.full_size, FULL_MIN)
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                min[0], min[1],
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if want.1 {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        }));
        self.applied = want;
    }
}

/// The size of the expanded window, in points.
pub const FULL_SIZE: [f32; 2] = [1500.0, 920.0];
/// The least the expanded window can be made, in points.
pub const FULL_MIN: [f32; 2] = [900.0, 600.0];
/// The size of the compact window, in points.
pub const COMPACT_SIZE: [f32; 2] = [420.0, 380.0];
/// The least the compact window can be made, in points.
pub const COMPACT_MIN: [f32; 2] = [360.0, 260.0];

/// What was clicked in the tab bar in one frame.
#[derive(Default)]
struct TabBar {
    select_record: bool,
    select: Option<usize>,
    close: Option<usize>,
    new_tab: bool,
}

/// What the Record tab says about itself in the bar.
struct RecordTab {
    label: String,
    recording: bool,
    selected: bool,
}

/// What a tab says about itself: its label and the longer text for hovering over it.
fn tab_title(screen: &Screen) -> (String, String) {
    match screen {
        Screen::Empty => ("New tab".to_string(), "Open a recording".to_string()),
        Screen::Loading { dir, .. } => (
            format!("… {}", tab_name(dir)),
            format!("opening {}", dir.display()),
        ),
        Screen::Failed { dir, message } => (
            format!("⚠ {}", tab_name(dir)),
            format!("{} could not be opened: {message}", dir.display()),
        ),
        Screen::Open(open) => (
            tab_name(&open.loaded.dir),
            open.loaded.dir.display().to_string(),
        ),
    }
}

/// Draws the Record tab, which has no close button, then the session tabs with one on each, and
/// the `+` after them. `active` is the session tab showing, if the Record tab is not.
fn tab_bar(ui: &mut egui::Ui, record: &RecordTab, tabs: &[Screen], active: usize) -> TabBar {
    let mut bar = TabBar::default();
    let text = egui::RichText::new(&record.label);
    let text = if record.recording {
        text.color(theme::BUG).strong()
    } else {
        text
    };
    if ui
        .selectable_label(record.selected, text)
        .on_hover_text("Record: start and stop a recording")
        .clicked()
    {
        bar.select_record = true;
    }
    ui.separator();
    let active = (!record.selected).then_some(active);
    let room = (ui.available_width() - 110.0).max(120.0);
    egui::ScrollArea::horizontal()
        .id_salt("tabs")
        .max_width(room)
        .auto_shrink([true, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (index, tab) in tabs.iter().enumerate() {
                    let (label, tip) = tab_title(tab);
                    let response = ui
                        .selectable_label(Some(index) == active, label)
                        .on_hover_text(tip);
                    if response.clicked() {
                        bar.select = Some(index);
                    }
                    if response.middle_clicked() {
                        bar.close = Some(index);
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Close the tab")
                        .clicked()
                    {
                        bar.close = Some(index);
                    }
                    ui.add_space(6.0);
                }
            });
        });
    if ui.button("+").on_hover_text("A new tab").clicked() {
        bar.new_tab = true;
    }
    bar
}

/// What to show before a session is open (or when it would not open).
fn start_screen(
    ui: &mut egui::Ui,
    prefs: &Prefs,
    failure: Option<(&Path, &str)>,
) -> Option<PathBuf> {
    let mut chosen = None;
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.heading("komorebi-recorder viewer");
        ui.add_space(6.0);
        ui.label("Open a recording's folder to play it and see everything that happened in it.");
        ui.add_space(16.0);

        if let Some((dir, message)) = failure {
            ui.colored_label(theme::BUG, format!("{} could not be opened:", name_of(dir)));
            ui.label(message);
            ui.add_space(16.0);
        }

        let recent = prefs.existing();
        if !recent.is_empty() {
            ui.label(egui::RichText::new("Recent").color(theme::TEXT_DIM));
            for dir in recent {
                if ui.link(dir.display().to_string()).clicked() {
                    chosen = Some(dir);
                }
            }
        }
    });
    chosen
}

impl Drop for Open {
    fn drop(&mut self) {
        self.changes_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Open {
    /// Returns the playhead, in session video time.
    #[must_use]
    pub fn position(&self) -> f64 {
        self.playback.position()
    }

    /// Shows `text` in the top bar for four seconds, as a note about what was just done.
    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }

    /// Everything that sits in the bar along the top once a session is open.
    fn top_bar(&mut self, ui: &mut egui::Ui, _prefs: &mut Prefs) {
        let manifest = &self.loaded.analysis.manifest;
        ui.label(
            egui::RichText::new(format!(
                "{} displays · {} events · {} s",
                self.loaded.displays.len(),
                self.loaded.events.len(),
                self.loaded.duration_s.round() as i64
            ))
            .color(theme::TEXT_DIM),
        );
        let versions: Vec<String> = manifest
            .komorebi
            .versions
            .iter()
            .filter_map(|version| {
                version
                    .commit
                    .as_ref()
                    .map(|commit| commit[..commit.len().min(7)].to_string())
            })
            .collect();
        if !versions.is_empty() {
            ui.label(
                egui::RichText::new(format!("komorebi {}", versions.join(", ")))
                    .color(theme::TEXT_DIM),
            );
        }
        if let Some(warning) = &self.loaded.skew.warning {
            let label = ui.colored_label(egui::Color32::from_rgb(235, 185, 60), "⚠ source differs");
            label.on_hover_text(warning);
        }
        if !self.loaded.warnings.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(235, 185, 60), "⚠")
                .on_hover_text(self.loaded.warnings.join("\n"));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("?").on_hover_text("Keys").clicked() {
                self.help = !self.help;
            }
            ui.checkbox(&mut self.overlay, "state overlay");
            if let Some((text, at)) = &self.message
                && at.elapsed() < Duration::from_secs(4)
            {
                ui.label(egui::RichText::new(text).color(theme::OK));
            }
        });
    }

    /// Carries out what a key asked for.
    ///
    /// | action | what happens |
    /// |---|---|
    /// | [`Action::TogglePlay`], [`Action::Pause`] | the playhead plays or pauses |
    /// | [`Action::PlayForwards`], [`Action::PlayBackwards`] | plays at [`keymap::shuttle`] of the current rate |
    /// | [`Action::StepFrame`] | one frame of the focused display (see [`Playback::step`]); with no display focused, pauses and moves a thirtieth of a second |
    /// | [`Action::Skip`] | moves that many seconds, without pausing |
    /// | [`Action::ToStart`], [`Action::ToEnd`] | moves to `0` or to the end |
    /// | [`Action::PreviousEvent`], [`Action::NextEvent`] | selects and goes to the event before or after the playhead |
    /// | [`Action::PreviousBug`], [`Action::NextBug`] | moves to the start of the bug region before or after the playhead |
    /// | [`Action::SetIn`], [`Action::SetOut`] | sets the start or the end of the selection to the playhead; a missing end is the end of the recording, a missing start is `0`, and the other end moves with the playhead if it would cross it |
    /// | [`Action::ClearSelection`] | clears the selection and stops looping |
    /// | [`Action::MarkBug`] | opens the form for a bug region |
    /// | [`Action::ToggleLoop`] | stops looping, or, with a selection, moves to its start and loops it, playing forwards at the current speed; with neither says to select a range first |
    /// | [`Action::ToggleOverlay`], [`Action::Help`] | switches the overlay, or the list of keys |
    /// | [`Action::ZoomIn`], [`Action::ZoomOut`] | zooms the timeline by a factor of 1.5 around the playhead |
    /// | [`Action::FitTimeline`] | shows the whole recording |
    /// | [`Action::FocusDisplay`] | focuses that display; an index beyond the displays does nothing |
    /// | [`Action::Escape`] | closes the list of keys if it is open, else the form for a bug region, else clears the selection and stops looping |
    ///
    /// Positions that fall outside the recording are clamped by the playhead (see
    /// [`Transport::seek`](crate::playback::Transport::seek)), so a skip past an end stops at it.
    pub fn apply(&mut self, action: Action) {
        let position = self.position();
        let duration = self.loaded.duration_s;
        match action {
            Action::TogglePlay => self.playback.toggle(),
            Action::Pause => self.playback.pause(),
            Action::PlayForwards | Action::PlayBackwards => {
                let forwards = action == Action::PlayForwards;
                let rate =
                    keymap::shuttle(self.playback.rate(), self.playback.is_playing(), forwards);
                self.playback.play(rate);
            }
            Action::StepFrame(direction) => {
                if let Some(display) = self.focus {
                    self.playback.step(display, direction);
                } else {
                    self.playback.pause();
                    self.playback.seek(position + f64::from(direction) / 30.0);
                }
            }
            Action::Skip(seconds) => self.playback.seek(position + f64::from(seconds)),
            Action::ToStart => self.playback.seek(0.0),
            Action::ToEnd => self.playback.seek(duration),
            Action::PreviousEvent => {
                if let Some(time) = timeline::previous_before(&self.loaded.event_times, position) {
                    self.select_event_at(time);
                }
            }
            Action::NextEvent => {
                if let Some(time) = timeline::next_after(&self.loaded.event_times, position) {
                    self.select_event_at(time);
                }
            }
            Action::PreviousBug => {
                let starts: Vec<f64> = self.loaded.bugs().map(|bug| bug.start_video_s).collect();
                if let Some(time) = timeline::previous_before(&starts, position) {
                    self.playback.seek(time);
                }
            }
            Action::NextBug => {
                let starts: Vec<f64> = self.loaded.bugs().map(|bug| bug.start_video_s).collect();
                if let Some(time) = timeline::next_after(&starts, position) {
                    self.playback.seek(time);
                }
            }
            Action::SetIn => {
                let end = self
                    .selection
                    .map_or(duration, |(_, end)| end.max(position));
                self.selection = Some((position, end));
            }
            Action::SetOut => {
                let start = self.selection.map_or(0.0, |(start, _)| start.min(position));
                self.selection = Some((start, position));
            }
            Action::ClearSelection => {
                self.selection = None;
                self.playback.set_loop(None);
            }
            Action::MarkBug => self.open_composer(),
            Action::ToggleLoop => {
                if self.playback.looping().is_some() {
                    self.playback.set_loop(None);
                    self.say("loop off");
                } else if let Some(selection) = self.selection {
                    self.playback.seek(selection.0);
                    self.playback.set_loop(Some(selection));
                    self.playback.play(self.playback.rate().abs());
                    self.say("looping the selection");
                } else {
                    self.say("select a range first: [ and ] mark its ends");
                }
            }
            Action::ToggleOverlay => self.overlay = !self.overlay,
            Action::ZoomIn => self.view.zoom(position, 1.5, duration),
            Action::ZoomOut => self.view.zoom(position, 1.0 / 1.5, duration),
            Action::FitTimeline => self.view = View::whole(duration),
            Action::FocusDisplay(index) => {
                if let Some(display) = self.loaded.displays.get(index) {
                    self.focus = Some(display.id);
                }
            }
            Action::Help => self.help = !self.help,
            // the tabs belong to the window (`ViewerApp::apply_tab_action`)
            Action::NewTab
            | Action::CloseTab
            | Action::NextTab
            | Action::PreviousTab
            | Action::GoToTab(_)
            | Action::LastTab
            | Action::ShowRecord => {}
            Action::Escape => {
                if self.help {
                    self.help = false;
                } else if self.composer.is_some() {
                    self.composer = None;
                } else if self.rule.is_some() && !self.is_sending() {
                    self.rule = None;
                } else {
                    self.selection = None;
                    self.playback.set_loop(None);
                }
            }
        }
    }

    /// Moves the playhead to the event received at `time` and selects it.
    ///
    /// The event is the one nearest `time`, within a millisecond; if there is none, nothing
    /// happens. See [`Open::select_event`].
    pub fn select_event_at(&mut self, time: f64) {
        let Some(index) = timeline::nearest(&self.loaded.event_times, time, 1e-3) else {
            return;
        };
        self.select_event(index);
    }

    /// Selects event number `index` of [`Loaded::events`] and goes to when it was received.
    ///
    /// It also shows the Event tab, pauses, and scrolls the timeline to keep the event in view. An
    /// `index` beyond the list does nothing.
    pub fn select_event(&mut self, index: usize) {
        let Some(event) = self.loaded.events.get(index) else {
            return;
        };

        self.selected_event = Some(event.seq);
        self.tab = Tab::Event;
        self.playback.pause();
        self.playback.seek(event.video_s);
        self.view.reveal(event.video_s, self.loaded.duration_s);
    }

    /// Read the keyboard, unless a text box wants it.
    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }

        // ⌘Z brings back what was deleted
        if ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::Z)) {
            self.undo_delete();
        }

        let actions: Vec<Action> = ctx.input(|input| {
            let typed: Vec<String> = input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Text(text) => Some(text.clone()),
                    _ => None,
                })
                .collect();
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        let text = typed.first().map(String::as_str);
                        keymap::action(*key, *modifiers, text)
                    }
                    _ => None,
                })
                .collect()
        });

        for action in actions {
            self.apply(action);
        }
    }

    /// Starts looking for where the picture changed, off the UI thread.
    ///
    /// The result arrives in [`Open::changes_result`] and is moved into [`Open::changes`] by a
    /// later frame. A session with no video has nothing to scan: nothing starts. The scan reads the
    /// video of every display from the start to the end of the recording, at the default
    /// sensitivity (see [`Analysis::changes_until`](crate::analysis::Analysis::changes_until)), and
    /// stops early when the session is closed (the [`Open`] is dropped).
    pub fn scan_for_changes(&mut self) {
        if self.loaded.displays.is_empty() {
            return;
        }

        let (sender, receiver) = crossbeam_channel::bounded(1);
        let analysis = self.loaded.analysis.clone();
        let displays: Vec<u32> = self
            .loaded
            .displays
            .iter()
            .map(|display| display.id)
            .collect();
        let duration = self.loaded.duration_s;
        let cancel = self.changes_cancel.clone();
        std::thread::spawn(move || {
            let outcome = SessionPath::new(&analysis.session.dir).and_then(|guard| {
                let video = crate::analysis::video::SessionVideo::new(&analysis.manifest, &guard);
                analysis.changes_until(&video, &displays, 0.0, duration, 1.0, Some(&cancel))
            });
            let _ = sender.send(outcome);
        });
        self.changes_result = Some(receiver);
    }

    /// Take the result of the background scan when it arrives.
    fn poll_changes(&mut self) {
        let Some(receiver) = &self.changes_result else {
            return;
        };
        let Ok(outcome) = receiver.try_recv() else {
            return;
        };

        self.changes_result = None;
        match outcome {
            Ok(data) => self.changes = Some(data),
            Err(error) => self.say(format!("could not look for screen changes: {error:#}")),
        }
    }

    /// Draws the open session: the panels, the form and the list of keys, and reads the keyboard.
    ///
    /// The panels are the timeline along the bottom, the inspector on the right and the video in the
    /// middle. While the playhead is moving the timeline follows it and the window asks to be drawn
    /// again at once; paused, it asks again in 100 ms, enough to bring in decoded frames and the
    /// result of the scan. Needs an `egui::Context`, so a window is needed.
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.poll_changes();
        if self.changes_result.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        self.keys(ctx);

        let playing = self.playback.is_playing();
        let position = self.position();
        if playing {
            // keep the window with the playhead while it plays
            self.view.reveal(position, self.loaded.duration_s);
        }

        self.timeline_panel(ctx);
        self.inspector_panel(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BACKGROUND)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ctx, |ui| self.video_ui(ui, ctx));

        self.composer_window(ctx);
        self.rule_window(ctx);
        if self.help {
            self.help_window(ctx);
        }

        if playing {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new("Keys")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("keys")
                    .num_columns(2)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        for (keys, does) in keymap::BINDINGS {
                            ui.monospace(*keys);
                            ui.label(*does);
                            ui.end_row();
                        }
                    });
            });
        if !open {
            self.help = false;
        }
    }

    /// Returns the state komorebi was in at session video time `time`, for the overlay.
    ///
    /// The state is the one in force at that moment, which is the state of the last notification
    /// before it, so it is kept in [`Open::state`] while the playhead stays within the same
    /// notification and not worked out again. `None` if the state at `time` cannot be read.
    pub fn state_at(&mut self, time: f64) -> Option<Arc<Snapshot>> {
        let mach = self.loaded.analysis.manifest.mach_at_video_seconds(time);
        let found = self.loaded.analysis.state_at(mach).ok()?;
        if let Some((id, snapshot)) = &self.state
            && found.state_id == Some(*id)
        {
            return Some(snapshot.clone());
        }

        if let Some(id) = found.state_id {
            self.state = Some((id, found.snapshot.clone()));
        }
        Some(found.snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations;
    use crate::testkit;
    use crate::testkit::SessionBuilder;
    use serde_json::json;

    fn prefs_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-viewer-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500.0, 920.0),
            )),
            events,
            ..egui::RawInput::default()
        }
    }

    fn frame(app: &mut ViewerApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let _ = ctx.run(input(events), |ctx| app.ui(ctx));
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn typed(character: &str, which: egui::Key) -> Vec<egui::Event> {
        vec![key(which), egui::Event::Text(character.to_string())]
    }

    fn open(app: &mut ViewerApp) -> &mut Open {
        match app.screen_mut() {
            Some(Screen::Open(open)) => open,
            _ => panic!("no session is open"),
        }
    }

    fn session(name: &str, with_video: bool) -> PathBuf {
        let mut builder = SessionBuilder::new(name)
            .event(
                2.0,
                json!({"type": "Retile"}),
                Some(testkit::state_with_windows(&[(1, "Ghostty"), (2, "Brave")])),
            )
            .event(
                4.0,
                json!({"type": "FocusChange", "content": [{"type": "Accessibility", "content": "AXApplicationActivated"}, 5, 1]}),
                Some(testkit::state_with_windows(&[(1, "Ghostty"), (2, "Brave")])),
            )
            .log(3.0, "ERROR", "komorebi::macos_api: failed to get window position: AXError::InvalidUIElement")
            .incident(5.0, "display-disconnected", "display 3 went away");
        if with_video {
            builder = builder.video(1, 0.0, 90, 30);
        }
        builder.build().0.dir
    }

    #[test]
    fn the_window_draws_with_nothing_open() {
        let mut app = ViewerApp::new(prefs_dir("empty"));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        assert!(
            app.record_active(),
            "the Record tab shows, not a start screen"
        );
        assert!(app.tabs().is_empty());
        assert!(app.screen().is_none());
    }

    #[test]
    fn a_session_that_will_not_open_says_why_on_the_start_screen() {
        let (session, mut manifest) = SessionBuilder::new("viewer-unfinished").build();
        manifest.status = "recording".to_string();
        session.write_manifest(&manifest).expect("manifest");

        let mut app = ViewerApp::new(prefs_dir("failed"));
        let ctx = egui::Context::default();
        app.open(&session.dir);

        let deadline = Instant::now() + Duration::from_secs(5);
        while matches!(app.screen(), Some(Screen::Loading { .. })) && Instant::now() < deadline {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(20));
        }
        let Some(Screen::Failed { message, .. }) = app.screen() else {
            panic!("the unfinished session opened");
        };
        assert!(message.contains("left unfinished"), "{message}");
        frame(&mut app, &ctx, vec![]);
    }

    #[test]
    fn an_opened_session_is_remembered() {
        let dir = session("viewer-remember", false);
        let prefs = prefs_dir("remember");
        let mut app = ViewerApp::new(prefs.clone());
        app.open_now(&dir).expect("open");

        assert_eq!(
            Prefs::load(&prefs).recent,
            [canonical(&dir).display().to_string()]
        );
    }

    #[test]
    fn a_session_draws_and_the_pictures_arrive() {
        if !testkit::ffmpeg_available() {
            eprintln!("skipped: ffmpeg is not available to make a test video");
            return;
        }

        let dir = session("viewer-draw", true);
        let mut app = ViewerApp::new(prefs_dir("draw"));
        app.open_now(&dir).expect("open");
        let ctx = egui::Context::default();

        // the picture for display 1 is decoded off the UI thread and turns up in a later frame
        let deadline = Instant::now() + Duration::from_secs(8);
        while open(&mut app).tiles.is_empty() && Instant::now() < deadline {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            open(&mut app).tiles.contains_key(&1),
            "display 1's picture never arrived"
        );
        assert_eq!(open(&mut app).focus, Some(1));
    }

    #[test]
    fn screen_changes_are_found_in_the_background_and_shown() {
        if !testkit::ffmpeg_available() {
            eprintln!("skipped: ffmpeg is not available to make a test video");
            return;
        }

        let (session, _) = SessionBuilder::new("viewer-changes")
            .video_showing(1, 0.0, 90, 30, |frame| if frame < 30 { frame } else { 29 })
            .build();
        let mut app = ViewerApp::new(prefs_dir("changes"));
        app.open_now(&session.dir).expect("open");
        let ctx = egui::Context::default();

        let deadline = Instant::now() + Duration::from_secs(10);
        while open(&mut app).changes.is_none() && Instant::now() < deadline {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(30));
        }
        let changes = open(&mut app).changes.clone().expect("the scan finished");
        assert_eq!(changes.spans.len(), 1, "{changes:#?}");
        assert!(changes.spans[0].end_video_s < 1.1);
        frame(&mut app, &ctx, vec![]);
    }

    /// A Windows session in which komorebi tiles a window that Windows says is cloaked on every
    /// read of it, with a bug marked from 1.5 s to 3.5 s and another from 20 s to 21 s.
    fn ghost_session(name: &str) -> PathBuf {
        use crate::analysis::flavor::Flavor;
        use crate::sources::winprobe::WindowFacts;

        const GHOST: i64 = 131_072;
        let window = || testkit::windows_window(GHOST, "DDM.exe", Some([0, 0, 960, 1040]));
        let state = || testkit::windows_state(65_537, vec![vec![window()]]);
        let event = json!({"type": "FocusChange", "content": ["SystemForeground", window()]});
        let cloaked = WindowFacts {
            hwnd: GHOST as u32,
            at_ns: 0,
            exists: true,
            visible: true,
            cloaked: true,
            minimized: false,
            rect: Some([0, 0, 960, 1040]),
            komorebi_says: None,
        };
        let (session, manifest) = SessionBuilder::new(name)
            .flavor(Flavor::Windows)
            .event(2.0, event.clone(), Some(state()))
            .probe(cloaked.clone())
            .event(3.0, event, Some(state()))
            .probe(cloaked)
            .build();

        for (from, to, text) in [(1.5, 3.5, "an empty tile"), (20.0, 21.0, "something else")] {
            annotations::add(
                &session.dir,
                annotations::New {
                    kind: Some(annotations::Kind::Bug),
                    start_mach_ns: manifest.mach_at_video_seconds(from),
                    end_mach_ns: Some(manifest.mach_at_video_seconds(to)),
                    text: text.to_string(),
                    ..annotations::New::default()
                },
            )
            .expect("bug");
        }
        session.dir
    }

    #[test]
    fn a_bug_marked_while_a_ghost_tile_was_there_is_offered_a_rule() {
        let dir = ghost_session("viewer-ghost");
        let mut app = ViewerApp::new(prefs_dir("ghost"));
        app.open_now(&dir).expect("open");
        let ctx = egui::Context::default();

        assert_eq!(open(&mut app).loaded.ghosts.len(), 1);
        let ghost = open(&mut app).loaded.ghosts[0].clone();
        assert_eq!(
            (ghost.exe.as_str(), ghost.class.as_str()),
            ("DDM.exe", "Window")
        );

        assert_eq!(
            open(&mut app).ghosts_during(1.5, 3.5),
            std::slice::from_ref(&ghost)
        );
        assert!(open(&mut app).ghosts_during(20.0, 21.0).is_empty());

        // the Bugs tab draws the card, and its button opens the form
        open(&mut app).tab = Tab::Bugs;
        frame(&mut app, &ctx, vec![]);
        open(&mut app).open_rule_form(ghost.hwnd);
        let form = open(&mut app).rule.as_ref().expect("the form is open");
        assert_eq!(form.app, "DDM");
        let rule = form.rule().expect("a rule");
        assert_eq!(
            rule.identifiers.len(),
            2,
            "the program and the class together"
        );
        frame(&mut app, &ctx, vec![]);

        // a blank name has no rule to send, and sending then does nothing
        open(&mut app).rule.as_mut().expect("form").app = "  ".to_string();
        assert!(open(&mut app).rule.as_ref().expect("form").rule().is_err());
        open(&mut app).start_sending(|| unreachable!("there is nothing to send"));
        assert!(!open(&mut app).is_sending());

        // and Escape closes the form
        frame(&mut app, &ctx, vec![key(egui::Key::Escape)]);
        assert!(open(&mut app).rule.is_none());
    }

    #[test]
    fn a_rule_is_sent_in_the_background_and_what_came_of_it_is_shown() {
        use crate::asc::submit::Outcome;
        use crate::asc::submit::Tools;
        use crate::asc::submit::Upstream;
        use crate::viewer::rules::Stage;

        let dir = ghost_session("viewer-send");
        let mut app = ViewerApp::new(prefs_dir("send"));
        app.open_now(&dir).expect("open");
        let ctx = egui::Context::default();
        let hwnd = open(&mut app).loaded.ghosts[0].hwnd;
        open(&mut app).open_rule_form(hwnd);

        // to a repository that is not there: nothing is sent anywhere
        let nowhere = std::env::temp_dir().join(format!(
            "komorebi-recorder-viewer-no-such-repository-{}",
            std::process::id()
        ));
        let upstream = Upstream {
            url: nowhere.to_string_lossy().into_owned(),
            ..Upstream::komorebi_asc()
        };
        open(&mut app).start_sending(move || {
            (
                Tools {
                    git: "git".into(),
                    gh: None,
                    env: Vec::new(),
                },
                upstream,
            )
        });
        assert!(open(&mut app).is_sending());
        // pressing it again while it goes does nothing more
        open(&mut app).start_sending(|| unreachable!("it is being sent"));

        let deadline = Instant::now() + Duration::from_secs(30);
        while open(&mut app).is_sending() && Instant::now() < deadline {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(30));
        }

        let form = open(&mut app).rule.take().expect("the form is still open");
        let Stage::Done(Outcome::Failed(failure)) = form.stage else {
            panic!(
                "it did not fail as it should: done is {}",
                matches!(form.stage, Stage::Done(_))
            );
        };
        assert_eq!(failure.step, "cloning the repository");
        assert!(
            failure.instructions.contains("DDMWindow")
                || failure.instructions.contains("\"Window\"")
        );
        if let Some(kept) = &failure.dir {
            let _ = std::fs::remove_dir_all(kept);
        }

        // what the form says when it is over draws
        open(&mut app).rule = Some(crate::viewer::rules::RuleForm {
            stage: Stage::Done(Outcome::Failed(failure.clone())),
            ..form
        });
        frame(&mut app, &ctx, vec![]);
    }

    #[test]
    fn the_keys_drive_the_playhead() {
        let dir = session("viewer-keys", false);
        let mut app = ViewerApp::new(prefs_dir("keys"));
        app.open_now(&dir).expect("open");
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);

        // space plays and pauses
        frame(&mut app, &ctx, vec![key(egui::Key::Space)]);
        assert!(open(&mut app).playback.is_playing());
        frame(&mut app, &ctx, vec![key(egui::Key::Space)]);
        assert!(!open(&mut app).playback.is_playing());

        // the event keys land on the events and select them
        open(&mut app).playback.seek(0.0);
        frame(&mut app, &ctx, typed(".", egui::Key::Period));
        assert!((open(&mut app).position() - 2.0).abs() < 1e-3);
        assert_eq!(open(&mut app).selected_event, Some(1));
        frame(&mut app, &ctx, typed(".", egui::Key::Period));
        assert_eq!(open(&mut app).selected_event, Some(2));
        frame(&mut app, &ctx, typed(",", egui::Key::Comma));
        assert_eq!(open(&mut app).selected_event, Some(1));

        // brackets pick a range
        open(&mut app).playback.seek(3.0);
        frame(&mut app, &ctx, typed("[", egui::Key::OpenBracket));
        open(&mut app).playback.seek(6.0);
        frame(&mut app, &ctx, typed("]", egui::Key::CloseBracket));
        assert_eq!(open(&mut app).selection, Some((3.0, 6.0)));
        frame(&mut app, &ctx, vec![key(egui::Key::Escape)]);
        assert_eq!(open(&mut app).selection, None);

        // J goes backwards, L forwards, each press faster
        frame(&mut app, &ctx, vec![key(egui::Key::J)]);
        assert!(open(&mut app).playback.rate() < 0.0);
        frame(&mut app, &ctx, vec![key(egui::Key::L)]);
        assert!(open(&mut app).playback.rate() > 0.0);
        frame(&mut app, &ctx, vec![key(egui::Key::L)]);
        assert!((open(&mut app).playback.rate() - 2.0).abs() < 1e-9);
        frame(&mut app, &ctx, vec![key(egui::Key::K)]);
        assert!(!open(&mut app).playback.is_playing());

        // the help comes and goes
        frame(&mut app, &ctx, typed("?", egui::Key::Questionmark));
        assert!(open(&mut app).help);
        frame(&mut app, &ctx, vec![key(egui::Key::Escape)]);
        assert!(!open(&mut app).help);
    }

    #[test]
    fn typing_in_a_text_box_does_not_press_keys() {
        let dir = session("viewer-typing", false);
        let mut app = ViewerApp::new(prefs_dir("typing"));
        app.open_now(&dir).expect("open");
        let ctx = egui::Context::default();

        // while something wants the keyboard, Space is a space
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("a text box")));
        let _ = ctx.run(input(vec![]), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut text = String::new();
                let response =
                    ui.add(egui::TextEdit::singleline(&mut text).id(egui::Id::new("a text box")));
                response.request_focus();
            });
        });
        assert!(ctx.wants_keyboard_input());
        let _ = ctx.run(input(vec![key(egui::Key::Space)]), |ctx| {
            if let Some(Screen::Open(open)) = app.screen_mut() {
                open.ui(ctx);
            }
        });
        assert!(!open(&mut app).playback.is_playing());
    }

    #[test]
    fn marking_a_bug_from_a_selection_saves_a_region_anyone_can_read() {
        let dir = session("viewer-mark", false);
        let mut app = ViewerApp::new(prefs_dir("mark"));
        app.open_now(&dir).expect("open");
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);

        // pick a range with the brackets, then B opens the form from it
        open(&mut app).playback.seek(3.0);
        frame(&mut app, &ctx, typed("[", egui::Key::OpenBracket));
        open(&mut app).playback.seek(6.0);
        frame(&mut app, &ctx, typed("]", egui::Key::CloseBracket));
        frame(&mut app, &ctx, vec![key(egui::Key::B)]);

        let form = open(&mut app).composer.as_ref().expect("the form is open");
        assert_eq!(form.start, "00:03.000");
        assert_eq!(form.end, "00:06.000");
        frame(&mut app, &ctx, vec![]);

        {
            let form = open(&mut app).composer.as_mut().expect("form");
            form.text =
                "the Brave window vanishes instead of moving to the laptop screen".to_string();
            form.expected = "it should move".to_string();
        }
        open(&mut app).save_composer().expect("saved");

        let loaded = annotations::load(&dir).expect("load");
        let bug = loaded.bugs().next().expect("a bug region was written");
        assert_eq!(
            bug.text,
            "the Brave window vanishes instead of moving to the laptop screen"
        );
        assert_eq!(bug.expected.as_deref(), Some("it should move"));
        assert_eq!(bug.author, annotations::AUTHOR_USER);
        let manifest = &open(&mut app).loaded.analysis.manifest.clone();
        assert!((manifest.video_seconds(bug.start_mach_ns) - 3.0).abs() < 1e-3);
        assert!((manifest.video_seconds(bug.end_mach_ns.expect("end")) - 6.0).abs() < 1e-3);

        // the window shows it, closes the form and selects the new region
        assert!(open(&mut app).composer.is_none());
        assert_eq!(open(&mut app).loaded.bugs().count(), 1);
        assert_eq!(
            open(&mut app).selected_bug.as_deref(),
            Some(bug.id.as_str())
        );
        frame(&mut app, &ctx, vec![]);
    }

    #[test]
    fn a_region_the_form_cannot_make_sense_of_stays_open_and_says_why() {
        let dir = session("viewer-refuse", false);
        let mut app = ViewerApp::new(prefs_dir("refuse"));
        app.open_now(&dir).expect("open");
        open(&mut app).open_composer();

        // no description
        let error = open(&mut app).save_composer().expect_err("refused");
        assert!(error.contains("say what goes wrong"), "{error}");
        assert!(open(&mut app).composer.is_some());

        open(&mut app).composer.as_mut().expect("form").text = "wrong".to_string();
        for (start, end, expected) in [
            ("banana", "00:05.000", "the start is not a time"),
            ("00:05.000", "00:02.000", "the end is before the start"),
            ("00:01.000", "99:00.000", "outside the recording"),
        ] {
            let form = open(&mut app).composer.as_mut().expect("form");
            form.start = start.to_string();
            form.end = end.to_string();
            let error = open(&mut app).save_composer().expect_err("refused");
            assert!(error.contains(expected), "{start} to {end}: {error}");
        }

        assert!(
            annotations::load(&dir)
                .expect("load")
                .annotations
                .is_empty(),
            "nothing was written"
        );
    }

    #[test]
    fn a_region_can_be_open_ended() {
        let dir = session("viewer-open-ended", false);
        let mut app = ViewerApp::new(prefs_dir("open-ended"));
        app.open_now(&dir).expect("open");
        open(&mut app).open_composer();
        {
            let form = open(&mut app).composer.as_mut().expect("form");
            form.text = "still broken when I stopped".to_string();
            form.open_ended = true;
            form.end = "nonsense".to_string();
        }
        open(&mut app)
            .save_composer()
            .expect("saved without reading the end");

        let bug = annotations::load(&dir)
            .expect("load")
            .bugs()
            .next()
            .cloned()
            .expect("bug");
        assert!(bug.open_ended());
    }

    #[test]
    fn a_region_can_be_edited_dragged_deleted_and_brought_back() {
        let dir = session("viewer-edit", false);
        let mut app = ViewerApp::new(prefs_dir("edit"));
        app.open_now(&dir).expect("open");
        open(&mut app).selection = Some((2.0, 4.0));
        open(&mut app).open_composer();
        open(&mut app).composer.as_mut().expect("form").text = "first words".to_string();
        open(&mut app).save_composer().expect("saved");
        let id = open(&mut app).selected_bug.clone().expect("selected");

        // edit the words
        open(&mut app).open_composer_for(&id);
        assert_eq!(
            open(&mut app).composer.as_ref().expect("form").text,
            "first words"
        );
        open(&mut app).composer.as_mut().expect("form").text = "better words".to_string();
        open(&mut app).save_composer().expect("saved");
        assert_eq!(
            open(&mut app).loaded.bugs().next().expect("bug").text,
            "better words"
        );
        assert_eq!(
            open(&mut app).loaded.bugs().count(),
            1,
            "an edit is not a second region"
        );

        // drag the edges: the end can move, but not before the start
        open(&mut app)
            .move_bug_edge(&id, false, 5.0)
            .expect("end moved");
        open(&mut app)
            .move_bug_edge(&id, true, 1.0)
            .expect("start moved");
        let bug = open(&mut app).loaded.bugs().next().cloned().expect("bug");
        assert!((bug.start_video_s - 1.0).abs() < 1e-3 && (bug.end_video_s - 5.0).abs() < 1e-3);
        let error = open(&mut app)
            .move_bug_edge(&id, false, 0.5)
            .expect_err("refused");
        assert!(error.contains("cannot end before it starts"), "{error}");

        // delete it, and bring it back
        open(&mut app).delete_annotation(&id);
        assert_eq!(open(&mut app).loaded.bugs().count(), 0);
        open(&mut app).undo_delete();
        let back = open(&mut app)
            .loaded
            .bugs()
            .next()
            .cloned()
            .expect("it is back");
        assert_eq!(back.text, "better words");
        assert!((back.start_video_s - 1.0).abs() < 1e-3 && (back.end_video_s - 5.0).abs() < 1e-3);
    }

    #[test]
    fn every_tab_of_the_inspector_draws() {
        let dir = session("viewer-tabs", false);
        let mut app = ViewerApp::new(prefs_dir("tabs"));
        app.open_now(&dir).expect("open");
        let ctx = egui::Context::default();

        // an event selected so the Event tab has a trace to show, and a region marked
        open(&mut app).select_event(1);
        open(&mut app).selection = Some((1.0, 2.0));
        open(&mut app).open_composer();
        open(&mut app).composer.as_mut().expect("form").text = "x".to_string();
        open(&mut app).save_composer().expect("saved");

        for tab in [Tab::Now, Tab::Event, Tab::Findings, Tab::Bugs, Tab::Log] {
            open(&mut app).tab = tab;
            frame(&mut app, &ctx, vec![]);
        }

        // and with the form and the help open over them
        open(&mut app).open_composer();
        open(&mut app).help = true;
        frame(&mut app, &ctx, vec![]);
    }

    fn names(app: &ViewerApp) -> Vec<String> {
        app.tabs()
            .iter()
            .map(|tab| match tab {
                Screen::Empty => "empty".to_string(),
                Screen::Loading { .. } => "loading".to_string(),
                Screen::Failed { .. } => "failed".to_string(),
                Screen::Open(open) => short(&open.loaded.dir),
            })
            .collect()
    }

    /// The name a test gave a session, without what the test kit adds to keep sessions apart.
    fn short(dir: &Path) -> String {
        let name = name_of(dir);
        let name = name
            .strip_prefix("komorebi-recorder-testkit-")
            .unwrap_or(&name);
        name.rsplit_once('-')
            .map_or(name.to_string(), |(name, _)| name.to_string())
    }

    fn with_command(key: egui::Key, shift: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                command: true,
                ctrl: !cfg!(target_os = "macos"),
                shift,
                ..egui::Modifiers::NONE
            },
        }
    }

    fn ctrl_tab(shift: bool) -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: Some(egui::Key::Tab),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                shift,
                ..egui::Modifiers::NONE
            },
        }
    }

    fn settle(app: &mut ViewerApp, ctx: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app
            .tabs()
            .iter()
            .any(|tab| matches!(tab, Screen::Loading { .. }))
            && Instant::now() < deadline
        {
            frame(app, ctx, vec![]);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn opening_from_a_start_tab_loads_into_it_and_from_a_session_adds_a_tab() {
        let (a, b, c) = (
            session("viewer-tabs-a", false),
            session("viewer-tabs-b", false),
            session("viewer-tabs-c", false),
        );
        let mut app = ViewerApp::new(prefs_dir("tabs-open"));
        let ctx = egui::Context::default();

        app.open(&a);
        assert_eq!(
            app.tabs().len(),
            1,
            "the Record tab is not one a session loads into"
        );
        settle(&mut app, &ctx);
        assert_eq!(names(&app), ["viewer-tabs-a"]);

        app.open(&b);
        assert_eq!(app.tabs().len(), 2);
        assert_eq!(app.active_index(), 1, "the new tab is shown");
        settle(&mut app, &ctx);
        app.open_now(&c).expect("open");
        assert_eq!(
            names(&app),
            ["viewer-tabs-a", "viewer-tabs-b", "viewer-tabs-c"]
        );
        assert_eq!(app.active_index(), 2);
        frame(&mut app, &ctx, vec![]);

        // a new tab is a start screen, and a session opened from it goes into it
        app.new_tab();
        assert_eq!(app.tabs().len(), 4);
        assert!(matches!(app.screen(), Some(Screen::Empty)));
        frame(&mut app, &ctx, vec![]);
        app.open_now(&a).expect("already open");
        assert_eq!(app.tabs().len(), 4, "nothing was added");
        assert_eq!(app.active_index(), 0, "it focused the tab that has it");
        app.focus_tab(3);
        app.open_now(&session("viewer-tabs-d", false))
            .expect("open");
        assert_eq!(app.tabs().len(), 4);
        assert_eq!(app.active_index(), 3);
        assert_eq!(names(&app)[3], "viewer-tabs-d");
    }

    #[test]
    fn a_session_that_is_already_open_is_focused_not_opened_again() {
        let a = session("viewer-dedupe-a", false);
        let b = session("viewer-dedupe-b", false);
        let mut app = ViewerApp::new(prefs_dir("dedupe"));
        let ctx = egui::Context::default();
        app.open_now(&a).expect("open");
        app.open_now(&b).expect("open");
        assert_eq!(app.active_index(), 1);

        // by the same path, and by a path that reaches it another way
        app.open(&a);
        assert_eq!((app.tabs().len(), app.active_index()), (2, 0));
        let roundabout = a.join("..").join(name_of(&a));
        app.focus_tab(1);
        app.open(&roundabout);
        assert_eq!((app.tabs().len(), app.active_index()), (2, 0));
        settle(&mut app, &ctx);
        assert!(
            matches!(app.screen(), Some(Screen::Open(_))),
            "not reloaded"
        );
    }

    #[test]
    fn a_session_that_fails_keeps_its_tab_and_its_reason() {
        let (unfinished, mut manifest) = SessionBuilder::new("viewer-tabs-unfinished").build();
        manifest.status = "recording".to_string();
        unfinished.write_manifest(&manifest).expect("manifest");
        let good = session("viewer-tabs-good", false);

        let mut app = ViewerApp::new(prefs_dir("tabs-failed"));
        let ctx = egui::Context::default();
        app.open_now(&good).expect("open");
        app.open(&unfinished.dir);
        assert_eq!(app.tabs().len(), 2, "the open session is not replaced");
        settle(&mut app, &ctx);

        let Some(Screen::Failed { message, .. }) = app.screen() else {
            panic!("it should have failed");
        };
        assert!(message.contains("left unfinished"), "{message}");
        frame(&mut app, &ctx, vec![]);

        // the next session loads into the failed tab, as into any free one
        app.open_now(&session("viewer-tabs-next", false))
            .expect("open");
        assert_eq!(app.tabs().len(), 2);
        assert_eq!(names(&app), ["viewer-tabs-good", "viewer-tabs-next"]);
    }

    #[test]
    fn closing_tabs_keeps_one_and_picks_a_neighbour() {
        let a = session("viewer-close-a", false);
        let b = session("viewer-close-b", false);
        let c = session("viewer-close-c", false);
        let mut app = ViewerApp::new(prefs_dir("close"));
        let ctx = egui::Context::default();
        for dir in [&a, &b, &c] {
            app.open_now(dir).expect("open");
        }
        assert_eq!(app.active_index(), 2);

        // closing a tab before the active one keeps the same session showing
        app.close_tab(0);
        assert_eq!(names(&app), ["viewer-close-b", "viewer-close-c"]);
        assert_eq!(
            short(app.screen().and_then(Screen::dir).expect("dir")),
            "viewer-close-c"
        );
        // closing the active last tab shows the one before
        app.close_tab(1);
        assert_eq!(names(&app), ["viewer-close-b"]);
        assert_eq!(app.active_index(), 0);
        // closing the last session tab shows the Record tab, and the window draws it
        app.close_tab(0);
        assert!(app.record_active());
        assert!(app.tabs().is_empty());
        frame(&mut app, &ctx, vec![]);
        // an index that is not there does nothing
        app.close_tab(7);
        assert!(app.tabs().is_empty());
    }

    #[test]
    fn closing_a_session_cancels_its_scan() {
        let dir = session("viewer-close-scan", false);
        let mut app = ViewerApp::new(prefs_dir("close-scan"));
        app.open_now(&dir).expect("open");
        let cancel = open(&mut app).changes_cancel.clone();
        assert!(!cancel.load(std::sync::atomic::Ordering::Relaxed));
        app.close_tab(0);
        assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn the_tab_keys_switch_close_and_open_tabs() {
        let a = session("viewer-keys-a", false);
        let b = session("viewer-keys-b", false);
        let c = session("viewer-keys-c", false);
        let mut app = ViewerApp::new(prefs_dir("tab-keys"));
        let ctx = egui::Context::default();
        for dir in [&a, &b, &c] {
            app.open_now(dir).expect("open");
        }
        frame(&mut app, &ctx, vec![]);

        frame(&mut app, &ctx, vec![ctrl_tab(false)]);
        assert!(app.record_active(), "next goes round to the Record tab");
        frame(&mut app, &ctx, vec![ctrl_tab(true)]);
        assert_eq!(
            app.active_index(),
            2,
            "previous goes back round to the last"
        );
        assert!(!app.record_active());
        // egui moved the focus for the Tab, and the window lets go of it again
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![]);
        assert!(!ctx.wants_keyboard_input());
        frame(&mut app, &ctx, vec![with_command(egui::Key::Num2, false)]);
        assert_eq!(app.active_index(), 1);
        frame(&mut app, &ctx, vec![with_command(egui::Key::Num9, false)]);
        assert_eq!(app.active_index(), 2, "9 is the last");
        frame(&mut app, &ctx, vec![with_command(egui::Key::Num1, false)]);
        assert_eq!(app.active_index(), 0);
        frame(&mut app, &ctx, vec![with_command(egui::Key::Num8, false)]);
        assert_eq!(app.active_index(), 0, "there is no eighth tab");
        frame(&mut app, &ctx, vec![with_command(egui::Key::Num0, false)]);
        assert!(app.record_active(), "0 is the Record tab");
        frame(&mut app, &ctx, vec![with_command(egui::Key::Num1, false)]);
        assert!(!app.record_active());
        frame(
            &mut app,
            &ctx,
            vec![with_command(egui::Key::CloseBracket, true)],
        );
        assert_eq!(app.active_index(), 1);
        frame(
            &mut app,
            &ctx,
            vec![with_command(egui::Key::OpenBracket, true)],
        );
        assert_eq!(app.active_index(), 0);

        frame(&mut app, &ctx, vec![with_command(egui::Key::T, false)]);
        assert_eq!(app.tabs().len(), 4);
        assert!(matches!(app.screen(), Some(Screen::Empty)));
        // the start screen has no keys of its own, and the tab keys work there too
        frame(&mut app, &ctx, vec![with_command(egui::Key::W, false)]);
        assert_eq!(app.tabs().len(), 3);
        frame(&mut app, &ctx, vec![with_command(egui::Key::W, false)]);
        assert_eq!(app.tabs().len(), 2);
        assert_eq!(names(&app), ["viewer-keys-a", "viewer-keys-b"]);

        // a plain Space still plays, in the active tab only
        frame(&mut app, &ctx, vec![key(egui::Key::Space)]);
        assert!(open(&mut app).playback.is_playing());
    }

    #[test]
    fn typing_in_a_text_box_does_not_press_the_tab_keys() {
        let mut app = ViewerApp::new(prefs_dir("tab-keys-typing"));
        let ctx = egui::Context::default();
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("a text box")));
        let _ = ctx.run(input(vec![]), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut text = String::new();
                let response =
                    ui.add(egui::TextEdit::singleline(&mut text).id(egui::Id::new("a text box")));
                response.request_focus();
            });
        });
        assert!(ctx.wants_keyboard_input());
        let _ = ctx.run(input(vec![with_command(egui::Key::T, false)]), |ctx| {
            app.ui(ctx);
        });
        assert!(app.tabs().is_empty(), "no new tab was opened");
    }

    #[test]
    fn only_the_tab_in_view_plays() {
        let a = session("viewer-play-a", false);
        let b = session("viewer-play-b", false);
        let mut app = ViewerApp::new(prefs_dir("play"));
        let ctx = egui::Context::default();
        app.open_now(&a).expect("open");
        frame(&mut app, &ctx, vec![key(egui::Key::Space)]);
        assert!(open(&mut app).playback.is_playing());

        // opening another session hides the first, and pauses it
        app.open_now(&b).expect("open");
        let Screen::Open(first) = &app.tabs()[0] else {
            panic!("the first is open");
        };
        assert!(!first.playback.is_playing());

        // and one set playing while hidden is stopped by the next frame
        if let Screen::Open(first) = &mut app.tabs[0] {
            first.playback.play(1.0);
        }
        frame(&mut app, &ctx, vec![]);
        let Screen::Open(first) = &app.tabs()[0] else {
            panic!("the first is open");
        };
        assert!(!first.playback.is_playing());

        // each keeps its own place
        app.focus_tab(0);
        frame(&mut app, &ctx, vec![key(egui::Key::End)]);
        let end = open(&mut app).position();
        app.focus_tab(1);
        assert!(open(&mut app).position() < end);
    }

    #[test]
    fn a_request_from_another_process_opens_tabs_and_raises_the_window() {
        let a = session("viewer-request-a", false);
        let b = session("viewer-request-b", false);
        let mut app = ViewerApp::new(prefs_dir("request"));
        let ctx = egui::Context::default();

        let request = ipc::Request {
            open: vec![a.clone(), b.clone()],
            focus: true,
            ..ipc::Request::default()
        };
        assert!(app.handle_request(&request));
        settle(&mut app, &ctx);
        assert_eq!(names(&app), ["viewer-request-a", "viewer-request-b"]);

        // asking again for one that is open only focuses it
        let again = ipc::Request {
            open: vec![a],
            focus: false,
            ..ipc::Request::default()
        };
        assert!(!app.handle_request(&again));
        assert_eq!((app.tabs().len(), app.active_index()), (2, 0));

        // nothing to open: a start-screen tab, but not a second one
        assert!(app.handle_request(&ipc::Request::default()));
        assert_eq!(app.tabs().len(), 3);
        assert!(app.handle_request(&ipc::Request::default()));
        assert_eq!(app.tabs().len(), 3);
    }

    #[test]
    fn requests_over_the_socket_reach_the_window() {
        let dir = std::env::temp_dir().join(format!("kr-viewer-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let socket = dir.join("v.sock");
        let a = session("viewer-socket-a", false);

        let Ok(ipc::Claim::Serving(bound)) = ipc::claim(&socket, &ipc::Request::default()) else {
            panic!("nobody is serving");
        };
        let mut app = ViewerApp::new(prefs_dir("socket"));
        app.listen(bound.serve(|| {}));
        let ctx = egui::Context::default();

        let request = ipc::Request {
            open: vec![a],
            focus: true,
            ..ipc::Request::default()
        };
        assert_eq!(
            ipc::send(&socket, &request).expect("sent"),
            ipc::Delivery::Delivered
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !matches!(app.screen(), Some(Screen::Open(_))) && Instant::now() < deadline {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(names(&app), ["viewer-socket-a"]);

        // closing the viewer removes the socket
        drop(app);
        assert!(!socket.exists());
    }

    #[test]
    fn every_kind_of_tab_draws_in_the_bar() {
        let (unfinished, mut manifest) = SessionBuilder::new("viewer-bar-unfinished").build();
        manifest.status = "recording".to_string();
        unfinished.write_manifest(&manifest).expect("manifest");
        let mut app = ViewerApp::new(prefs_dir("bar"));
        let ctx = egui::Context::default();
        app.open_now(&session("viewer-bar-ok", false))
            .expect("open");
        app.open(&unfinished.dir);
        frame(&mut app, &ctx, vec![]);
        settle(&mut app, &ctx);
        app.new_tab();
        frame(&mut app, &ctx, vec![]);
        assert_eq!(
            names(&app),
            ["viewer-bar-ok", "failed", "empty"],
            "open, failed and start tabs"
        );
    }

    // ---- the Record tab and the one window ----

    /// An app whose recorder touches no screen, and the flag that stands for SIGINT.
    fn quiet(name: &str) -> (ViewerApp, &'static std::sync::atomic::AtomicBool) {
        let quit: &'static std::sync::atomic::AtomicBool =
            Box::leak(Box::new(std::sync::atomic::AtomicBool::new(false)));
        let mut app = ViewerApp::new(prefs_dir(name));
        *app.recorder_mut() = RecorderApp::quiet(quit);
        (app, quit)
    }

    /// A recording that is finished when it is asked to stop, and saves `dir`.
    fn fake_recording(
        live: Arc<crate::recorder::Live>,
        dir: PathBuf,
    ) -> std::thread::JoinHandle<eyre::Result<PathBuf>> {
        std::thread::spawn(move || {
            while !live.stop_requested() {
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(dir)
        })
    }

    /// Draws a frame and returns the commands it sent to the window.
    fn commands(
        app: &mut ViewerApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        close_requested: bool,
    ) -> Vec<egui::ViewportCommand> {
        let mut raw = input(events);
        if close_requested {
            raw.viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .events
                .push(egui::ViewportEvent::Close);
        }
        let output = ctx.run(raw, |ctx| app.ui(ctx));
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.commands.clone())
            .unwrap_or_default()
    }

    fn start_recording(app: &mut ViewerApp, phase: crate::recorder::Phase) {
        let live = app.recorder().live();
        let worker = fake_recording(live, PathBuf::from("saved-session"));
        app.recorder_mut().pretend(worker, phase, 12.0);
    }

    #[test]
    fn the_record_tab_is_first_pinned_and_cannot_be_closed() {
        let (a, b) = (
            session("viewer-rec-a", false),
            session("viewer-rec-b", false),
        );
        let (mut app, _) = quiet("record-first");
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        assert!(app.record_active() && app.tabs().is_empty());
        assert!(app.is_compact(), "a plain start is the small window");

        // a session is tab 1 (the first session tab), and the window expands to show it
        app.open_now(&a).expect("open");
        assert_eq!(app.tabs().len(), 1);
        assert!(!app.record_active());
        assert!(!app.is_compact(), "opening a session expands the window");
        app.open_now(&b).expect("open");
        frame(&mut app, &ctx, vec![]);

        // the key for the Record tab, and Cmd/Ctrl W on it does nothing
        frame(&mut app, &ctx, vec![with_command(egui::Key::Num0, false)]);
        assert!(app.record_active());
        frame(&mut app, &ctx, vec![with_command(egui::Key::W, false)]);
        assert!(app.record_active());
        assert_eq!(app.tabs().len(), 2, "no session was closed either");
        // the tab bar and the tab draw with the Record tab showing
        frame(&mut app, &ctx, vec![]);

        // `+` after the Record tab is a start screen; closing every session returns to Record
        app.new_tab();
        assert!(matches!(app.screen(), Some(Screen::Empty)));
        for _ in 0..3 {
            app.close_tab(0);
        }
        assert!(app.tabs().is_empty());
        assert!(app.record_active(), "not an empty start screen");
        frame(&mut app, &ctx, vec![]);
    }

    #[test]
    fn opening_a_session_expands_the_compact_window_and_asks_the_window_to_resize() {
        let a = session("viewer-expand-a", false);
        let (mut app, _) = quiet("expand");
        let ctx = egui::Context::default();
        assert!(app.is_compact() && app.is_on_top());
        // nothing to say while the window is as it was made
        let sent = commands(&mut app, &ctx, vec![], false);
        assert!(
            !sent
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::InnerSize(_)))
        );

        app.open_now(&a).expect("open");
        assert!(!app.is_compact() && !app.is_on_top());
        let sent = commands(&mut app, &ctx, vec![], false);
        assert!(
            sent.contains(&egui::ViewportCommand::InnerSize(egui::vec2(1500.0, 920.0))),
            "{sent:?}"
        );
        assert!(sent.contains(&egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::Normal
        )));

        // back to the small window: pinned on top again
        app.toggle_compact();
        assert!(app.is_compact() && app.record_active());
        let sent = commands(&mut app, &ctx, vec![], false);
        assert!(
            sent.contains(&egui::ViewportCommand::InnerSize(egui::vec2(420.0, 380.0))),
            "{sent:?}"
        );
        assert!(sent.contains(&egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop
        )));
        // and a key that shows a session expands it again
        frame(&mut app, &ctx, vec![with_command(egui::Key::Num1, false)]);
        assert!(!app.is_compact() && !app.record_active());
    }

    #[test]
    fn the_compact_choice_is_kept_in_the_preferences() {
        let dir = prefs_dir("compact-prefs");
        let mut app = ViewerApp::new(dir.clone());
        assert!(app.is_compact(), "the default is the small window");
        app.toggle_compact();
        assert!(!app.is_compact());
        assert!(!Prefs::load(&dir).compact);
        // the next start is the full window
        assert!(!ViewerApp::new(dir.clone()).is_compact());
        app.toggle_compact();
        assert!(Prefs::load(&dir).compact);

        // showing a session expands without being remembered
        let a = session("viewer-compact-a", false);
        app.open_now(&a).expect("open");
        assert!(!app.is_compact());
        assert!(Prefs::load(&dir).compact, "only the button writes it");

        // `view` with sessions starts full, and with none shows a start screen
        let launched = ViewerApp::start(dir.clone(), crate::viewer::Launch::sessions(Vec::new()));
        assert!(!launched.is_compact());
        assert_eq!(launched.tabs().len(), 1);
        assert!(matches!(launched.screen(), Some(Screen::Empty)));
        assert!(Prefs::load(&dir).compact);
        // an older preferences file has no such field, and keeps the small window
        std::fs::write(dir.join("viewer-prefs.json"), r#"{"overlay": true}"#).expect("write");
        assert!(ViewerApp::new(dir).is_compact());
    }

    #[test]
    fn the_tab_says_when_a_recording_is_going() {
        let a = session("viewer-marker-a", false);
        let (mut app, _) = quiet("marker");
        let ctx = egui::Context::default();
        assert_eq!(app.recorder().tab_label(), ("Record".to_string(), false));

        start_recording(&mut app, crate::recorder::Phase::Recording);
        let (label, recording) = app.recorder().tab_label();
        assert!(recording && label.starts_with("● REC "), "{label}");
        // it draws, in the bar and with a session showing, with the pin toggle
        app.open_now(&a).expect("open");
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![with_command(egui::Key::Num0, false)]);

        let live = app.recorder().live();
        live.request_stop();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.recorder().recording() && Instant::now() < deadline {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.recorder().tab_label(), ("Record".to_string(), false));
    }

    #[test]
    fn a_recording_is_joined_and_saved_while_another_tab_is_showing() {
        let a = session("viewer-poll-a", false);
        let (mut app, _) = quiet("poll");
        let ctx = egui::Context::default();
        start_recording(&mut app, crate::recorder::Phase::Recording);
        app.open_now(&a).expect("open");
        assert!(!app.record_active());

        let live = app.recorder().live();
        live.request_stop();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.recorder().recording() && Instant::now() < deadline {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.recorder().recording());
        assert_eq!(
            app.recorder().saved(),
            Some(Path::new("saved-session")),
            "the worker was joined and its session kept, with the Record tab hidden"
        );
        assert!(!app.record_active(), "and the tab in view was left alone");
    }

    #[test]
    fn closing_the_window_while_recording_stops_the_recording_first() {
        let a = session("viewer-close-rec-a", false);
        let (mut app, _) = quiet("close-recording");
        let ctx = egui::Context::default();
        start_recording(&mut app, crate::recorder::Phase::Recording);
        app.open_now(&a).expect("open");

        let sent = commands(&mut app, &ctx, vec![], true);
        assert!(
            sent.contains(&egui::ViewportCommand::CancelClose),
            "{sent:?}"
        );
        assert!(!sent.contains(&egui::ViewportCommand::Close));
        assert!(app.recorder().live().stop_requested());
        assert!(app.recorder().closing());

        // once the recording has finalized the window closes
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut closed = false;
        while !closed && Instant::now() < deadline {
            closed =
                commands(&mut app, &ctx, vec![], false).contains(&egui::ViewportCommand::Close);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(closed, "the window closed after the recording was done");
        assert!(app.recorder().saved().is_some());
    }

    #[test]
    fn a_quit_signal_while_recording_is_the_same_as_closing() {
        let (mut app, quit) = quiet("quit-recording");
        let ctx = egui::Context::default();
        start_recording(&mut app, crate::recorder::Phase::Recording);
        // with a session showing, as the other tests do
        app.open_now(&session("viewer-quit-a", false))
            .expect("open");
        quit.store(true, std::sync::atomic::Ordering::SeqCst);

        let sent = commands(&mut app, &ctx, vec![], false);
        assert!(
            sent.contains(&egui::ViewportCommand::CancelClose),
            "{sent:?}"
        );
        assert!(app.recorder().live().stop_requested());
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut closed = false;
        while !closed && Instant::now() < deadline {
            closed =
                commands(&mut app, &ctx, vec![], false).contains(&egui::ViewportCommand::Close);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(closed);
    }

    #[test]
    fn with_no_recording_a_close_request_closes_and_a_quit_signal_closes_too() {
        let (mut app, quit) = quiet("close-idle");
        let ctx = egui::Context::default();
        let sent = commands(&mut app, &ctx, vec![], true);
        assert!(
            !sent.contains(&egui::ViewportCommand::CancelClose),
            "{sent:?}"
        );

        quit.store(true, std::sync::atomic::Ordering::SeqCst);
        let sent = commands(&mut app, &ctx, vec![], false);
        assert!(sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    }

    #[test]
    fn a_gui_request_shows_the_record_tab_and_a_view_request_opens_tabs() {
        let (a, b) = (
            session("viewer-gui-a", false),
            session("viewer-gui-b", false),
        );
        let (mut app, _) = quiet("gui-request");
        let ctx = egui::Context::default();

        // `view SESSION...` forwarded: tabs, in the full window
        let view = ipc::Request {
            open: vec![a.clone(), b],
            ..ipc::Request::default()
        };
        assert!(app.handle_request(&view));
        settle(&mut app, &ctx);
        assert_eq!(names(&app), ["viewer-gui-a", "viewer-gui-b"]);
        assert!(!app.record_active() && !app.is_compact());

        // `gui` forwarded: the Record tab, and no tab added or closed
        let gui = ipc::Request {
            tab: Some(ipc::TabName::Record),
            ..ipc::Request::default()
        };
        assert!(app.handle_request(&gui), "the window is raised");
        assert!(app.record_active());
        assert_eq!(app.tabs().len(), 2);

        // sessions and the Record tab together open the sessions and end on Record
        let both = ipc::Request {
            open: vec![a],
            tab: Some(ipc::TabName::Record),
            ..ipc::Request::default()
        };
        assert!(app.handle_request(&both));
        assert!(app.record_active() && app.tabs().len() == 2);

        // a request in the old shape is still understood
        let old: ipc::Request =
            serde_json::from_str(r#"{"open":[],"focus":false}"#).expect("old request");
        assert!(!app.handle_request(&old));
        assert_eq!(
            app.tabs().len(),
            3,
            "an empty open is a start screen, as it was"
        );
        assert!(matches!(app.screen(), Some(Screen::Empty)));
    }

    #[test]
    fn a_gui_request_over_the_socket_reaches_a_hidden_record_tab() {
        let dir = std::env::temp_dir().join(format!("kr-viewer-gui-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let socket = dir.join("v.sock");
        let a = session("viewer-gui-socket-a", false);

        let Ok(ipc::Claim::Serving(bound)) = ipc::claim(&socket, &ipc::Request::default()) else {
            panic!("nobody is serving");
        };
        let (mut app, _) = quiet("gui-socket");
        app.listen(bound.serve(|| {}));
        app.open_now(&a).expect("open");
        assert!(!app.record_active());
        let ctx = egui::Context::default();

        let gui = ipc::Request {
            tab: Some(ipc::TabName::Record),
            ..ipc::Request::default()
        };
        assert!(matches!(
            ipc::claim(&socket, &gui).expect("claim"),
            ipc::Claim::Forwarded
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.record_active() && Instant::now() < deadline {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(app.record_active());
        assert_eq!(app.tabs().len(), 1);
    }

    #[test]
    fn the_agent_prompt_names_the_session_and_the_commands_to_start_with() {
        let prompt = super::super::inspector::agent_prompt(Path::new("/tmp/a session"));
        assert!(prompt.contains("/tmp/a session"));
        assert!(prompt.contains("komorebi-recorder brief"));
        assert!(prompt.contains("komorebi-recorder guide"));
        // the prompt says what is available, so an agent does not have to guess
        for command in [
            "lag",
            "measure",
            "motion",
            "trace",
            "--json",
            "komorebi-recorder schema",
        ] {
            assert!(
                prompt.contains(command),
                "the prompt does not mention {command}"
            );
        }
    }
}
