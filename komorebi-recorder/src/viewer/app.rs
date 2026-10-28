//! The viewer window: opening a session and keeping everything on screen in step.
//!
//! [`ViewerApp`] is the whole application and holds a [`Screen`]: nothing opened yet, a session
//! loading, a session that failed to open, or an [`Open`] session. [`ViewerApp::ui`] draws one frame
//! of whichever it is. An [`Open`] is where the state of a session being viewed lives (the
//! playhead's [`Playback`], the visible stretch of the timeline, the selection, which event or
//! finding or bug region is selected, the form being filled in), and its methods are spread
//! over the files that draw each part: this file keeps it in step with the keyboard, and the lanes,
//! the inspector, the video and the composer each add the methods that draw and act on their
//! part.
//!
//! Everything here needs an `egui::Context` to draw, so none of it has an example. The tests at the
//! bottom of this file run [`ViewerApp::ui`] headlessly through `egui::Context::run` with real key
//! events, against sessions made by the test kit, and are what covers the behavior.

use super::keymap;
use super::keymap::Action;
use super::loaded::Loaded;
use super::loaded::name_of;
use super::prefs::Prefs;
use super::theme;
use super::timeline;
use super::timeline::View;
use crate::analysis::guard::SessionPath;
use crate::analysis::state::Snapshot;
use crate::analysis::trace::TraceData;
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

/// The screen the window is showing: a session, or what stands in for one.
///
/// A [`ViewerApp`] is always on exactly one of these. [`ViewerApp::open`] moves it from `Empty`
/// (or any screen) to `Loading`, and from there to `Open` or `Failed` when the load finishes.
pub enum Screen {
    /// Nothing opened yet: the start screen, with the recent sessions that still exist.
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

/// The viewer application: the preferences and the screen being shown.
///
/// This is what [`run`](super::run) hands to `eframe`. [`ViewerApp::new`] makes one showing the
/// start screen, [`ViewerApp::open`] opens a session, and [`ViewerApp::ui`] draws a frame. It needs
/// a window to draw and so has no example.
pub struct ViewerApp {
    /// The viewer's preferences. Saved when a session is opened and when the state overlay is
    /// switched.
    pub prefs: Prefs,
    prefs_dir: PathBuf,
    /// The screen the window is showing.
    pub screen: Screen,
    picker: Option<Receiver<Option<PathBuf>>>,
}

/// Ask the user for a folder with the system's own dialog, off the UI thread.
fn pick_folder() -> Receiver<Option<PathBuf>> {
    let (sender, receiver) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = sender.send(crate::platform::shell::pick_folder("Choose a recording"));
    });
    receiver
}

impl ViewerApp {
    /// Returns a viewer showing the start screen, with the preferences kept in `prefs_dir`.
    ///
    /// The preferences are read with [`Prefs::load`], so a missing or damaged file gives the
    /// defaults. Nothing is opened: see [`ViewerApp::open`].
    #[must_use]
    pub fn new(prefs_dir: PathBuf) -> Self {
        Self {
            prefs: Prefs::load(&prefs_dir),
            prefs_dir,
            screen: Screen::Empty,
            picker: None,
        }
    }

    /// Starts opening the session in `dir` in the background.
    ///
    /// The screen becomes [`Screen::Loading`] at once, replacing whatever was showing, and
    /// [`ViewerApp::ui`] brings the result in when the load has finished: an [`Open`] session, or
    /// [`Screen::Failed`] with the reason. A session that cannot be opened is therefore not an error
    /// here; see [`Loaded::load`] for what can go wrong.
    pub fn open(&mut self, dir: &Path) {
        let (sender, result) = crossbeam_channel::bounded(1);
        let target = dir.to_path_buf();
        std::thread::spawn(move || {
            let _ = sender.send(Loaded::load(&target));
        });
        self.screen = Screen::Loading {
            dir: dir.to_path_buf(),
            result,
        };
    }

    /// Opens the session in `dir` and waits for it: for tests, which have nothing to draw
    /// meanwhile.
    ///
    /// On success the screen is [`Screen::Open`], the session is at the top of the recent
    /// sessions (and the preferences are saved), and the playhead is a second before where the
    /// first bug region starts, if there is one.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be loaded (see [`Loaded::load`]), if `dir` is not a
    /// session directory, or if the playback cannot be started (see [`Playback::start`]). The
    /// screen is left as it was: unlike [`ViewerApp::open`], a failure is not shown in the window.
    pub fn open_now(&mut self, dir: &Path) -> eyre::Result<()> {
        let loaded = Loaded::load(dir)?;
        self.finish_open(loaded)
    }

    fn finish_open(&mut self, loaded: Loaded) -> eyre::Result<()> {
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
        self.screen = Screen::Open(Box::new(open));
        Ok(())
    }

    /// Bring a finished load into the window, or show why it failed.
    fn poll_loading(&mut self) {
        let Screen::Loading { dir, result } = &self.screen else {
            return;
        };

        let Ok(outcome) = result.try_recv() else {
            return;
        };
        let dir = dir.clone();
        match outcome.and_then(|loaded| self.finish_open(loaded)) {
            Ok(()) => {}
            Err(error) => {
                self.screen = Screen::Failed {
                    dir,
                    message: format!("{error:#}"),
                };
            }
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

    /// Draws one frame of the window.
    ///
    /// It first takes in a finished load and a folder chosen in the system's dialog, then draws the
    /// top bar and the screen, and saves the preferences if the state overlay was switched. While a
    /// session is loading, or the dialog is open, it asks `ctx` to draw again soon, so the result
    /// shows without anyone touching the window. An open session draws itself with [`Open::ui`].
    ///
    /// Needs an `egui::Context`, so a window is needed (the tests of this module draw frames
    /// headlessly with `egui::Context::run`).
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.poll_loading();
        self.poll_picker();

        let mut to_open: Option<PathBuf> = None;
        let mut choose = false;

        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(theme::BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // leave room for the window controls that sit over the content
                    ui.add_space(70.0);
                    if ui.button("Open…").clicked() {
                        choose = true;
                    }
                    match &mut self.screen {
                        Screen::Open(open) => {
                            ui.label(egui::RichText::new(name_of(&open.loaded.dir)).strong());
                            open.top_bar(ui, &mut self.prefs);
                        }
                        Screen::Loading { dir, .. } => {
                            ui.spinner();
                            ui.label(format!("opening {}…", name_of(dir)));
                        }
                        _ => {}
                    }
                });
            });

        match &mut self.screen {
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
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }

        if choose && self.picker.is_none() {
            self.picker = Some(pick_folder());
        }
        if let Some(dir) = to_open {
            self.open(&dir);
        }
        if self.picker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        if let Screen::Open(open) = &self.screen
            && open.overlay != self.prefs.overlay
        {
            self.prefs.overlay = open.overlay;
            self.prefs.save(&self.prefs_dir);
        }
    }
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
        match &mut app.screen {
            Screen::Open(open) => open,
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
        assert!(matches!(app.screen, Screen::Empty));
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
        while matches!(app.screen, Screen::Loading { .. }) && Instant::now() < deadline {
            frame(&mut app, &ctx, vec![]);
            std::thread::sleep(Duration::from_millis(20));
        }
        let Screen::Failed { message, .. } = &app.screen else {
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

        assert_eq!(Prefs::load(&prefs).recent, [dir.display().to_string()]);
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
            if let Screen::Open(open) = &mut app.screen {
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

    #[test]
    fn the_agent_prompt_names_the_session_and_the_commands_to_start_with() {
        let prompt = super::super::inspector::agent_prompt(Path::new("/tmp/a session"));
        assert!(prompt.contains("/tmp/a session"));
        assert!(prompt.contains("komorebi-recorder brief"));
        assert!(prompt.contains("komorebi-recorder guide"));
    }
}
