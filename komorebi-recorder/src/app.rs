//! The recorder tab: Record / Stop, live counters and warnings.
//!
//! The recorder is not a window of its own. It is the pinned first tab of the one window of the
//! program ([`crate::viewer`]), which `komorebi-recorder` (with no command) and `komorebi-recorder
//! gui` open, and which also holds a tab for every session being reviewed. This module is the
//! recorder itself, [`RecorderApp`]: the host window draws it into the Record tab with
//! [`RecorderApp::ui`] and, on every frame whichever tab is showing, calls [`RecorderApp::poll`], so
//! that a recording carries on, and is finalized, while a session is being looked at. [`run`] opens
//! the window; it is the same recorder as `komorebi-recorder record`, with a window instead of
//! Ctrl-C.
//!
//! # What the tab does
//!
//! | control | effect |
//! |---|---|
//! | Record | starts a recording on a thread named `recorder` ([`recorder::record`]), with the options the window was started with, the displays ticked below and the predicate typed under Advanced |
//! | Stop | asks the recording to finish ([`Live::request_stop`]); it indexes and finalizes the session, which the tab shows as "finalizing..." |
//! | Open session..., Open last | open a recording as a new tab of this same window (a folder dialog, or the newest session); the window expands if it was compact |
//! | Compact / Expand | switches the window between the small control window and the full one ([`crate::viewer`]) |
//! | Copy path, Open in the file manager | after a recording: copy where the session was saved, or show it (Finder or Explorer) |
//!
//! While idle the list of displays is read again every second, so a display that was docked or
//! undocked appears or disappears and a new one starts ticked. With every display ticked the
//! recorder follows all of them, including any connected while recording; with one unticked it
//! records only the ticked ones. While recording the tab shows counters (video frames and the
//! frames dropped, the size of komorebi's log, the number of komorebi events, the OS trace and each
//! of its streams), the most recent incidents to the recording environment, and any warnings, and
//! the tab's label in the tab bar says `REC` and the time.
//!
//! The tab does not mark where a bug is: that is done afterwards, in a session's tab.
//!
//! Closing the window while recording, or delivering SIGINT or SIGTERM to the process (the handler
//! also runs on SIGHUP on Unix, and on Ctrl-C on Windows), finishes and indexes the session first
//! rather than leaving it half-written, and only then closes the window. With no recording the
//! window closes at once.
//!
//! # Staying out of the video and out of the layout
//!
//! The window is for use on the machine whose behavior is being recorded, so it must not appear in
//! the video and must not be tiled by komorebi. Both hold for the one window, in either size.
//!
//! - **Not in the video.** On macOS the capture filter excludes this whole application (this
//!   process and every process named `komorebi-recorder`), so the window never appears in the
//!   video. On Windows a whole monitor is captured, so about once a second the window asks Windows
//!   to leave it out of captures (`WDA_EXCLUDEFROMCAPTURE`) through
//!   [`crate::platform::window::Chrome`].
//! - **Not tiled.** The window is named only when it is safely ignored ([`crate::komorebi_ignore`]):
//!   on Windows, [`run`] first asks a running komorebi, with `komorebic ignore-rule exe`, to ignore
//!   the recorder's executables (komorebi's live rule, nothing is written to `komorebi.json`), and
//!   then titles the window `komorebi-recorder`; with no komorebi running it is titled too. If
//!   `komorebic` cannot be found or fails, and always on macOS (komorebi for Mac manages any window
//!   with a title, in `Window::should_manage`), the window stays untitled so that opening the
//!   recorder does not perturb the very layout being observed, and one line on standard error and
//!   the tab's message area say how to add the rule by hand. On Windows
//!   `KOMOREBI_RECORDER_PROBE_WINDOW=1` makes the window ask komorebi whether it would manage it
//!   (see [`crate::platform::window::PROBE_VARIABLE`]).
//!
//! The tab needs an `egui::Ui` to draw and a screen to record, so it has no example. Its logic that
//! does not need either is covered by this module's unit test
//! (`the_latest_session_is_the_last_by_name_and_must_be_a_session`), and the tests of
//! `viewer::app` drive it inside the window.

use crate::capture::DisplayDescriptor;
use crate::cli::RecordArgs;
use crate::platform::capture;
use crate::platform::shell;
use crate::recorder;
use crate::recorder::Live;
use crate::recorder::Phase;
use crate::timecode;
use color_eyre::eyre;
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

/// Returns the newest session in `recordings`: sessions are folders named for when they began, so
/// the last by name is the latest.
///
/// A folder counts as a session only if it has a session manifest
/// ([`MANIFEST_FILE`](crate::session::MANIFEST_FILE)): a newer folder that is not a session is
/// passed over. `None` if there is no session, or `recordings` cannot be read (it does not exist,
/// or is not a folder). The window offers the result as "Open last", with
/// [`default_recordings_dir`](crate::session::default_recordings_dir) as `recordings`.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::app::latest_session_in;
/// use komorebi_recorder::session::MANIFEST_FILE;
///
/// let name = format!("komorebi-recorder-doc-latest-{}", std::process::id());
/// let recordings = std::env::temp_dir().join(name);
/// for session in ["20261026T100000Z", "20261026T120000Z", "20261025T090000Z"] {
///     std::fs::create_dir_all(recordings.join(session))?;
///     std::fs::write(recordings.join(session).join(MANIFEST_FILE), "{}")?;
/// }
/// // a newer folder that is not a session does not count
/// std::fs::create_dir_all(recordings.join("20261027T000000Z"))?;
///
/// assert_eq!(
///     latest_session_in(&recordings),
///     Some(recordings.join("20261026T120000Z")),
/// );
/// assert_eq!(latest_session_in(&recordings.join("nothing-here")), None);
/// # std::fs::remove_dir_all(&recordings)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn latest_session_in(recordings: &std::path::Path) -> Option<PathBuf> {
    std::fs::read_dir(recordings)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join(crate::session::MANIFEST_FILE).is_file())
        .max()
}

/// Open `session` (or just a start-screen tab, with none) in the program's window, from another
/// process.
///
/// This is for a process that is not the window, such as a script or another tool: the window
/// itself opens sessions as tabs without any of this. If a window is already running it is asked,
/// over its socket, to open the session as a tab and to come to the front: no process is started.
/// Only when none answers is `komorebi-recorder view` started, in a process of its own, and kept out
/// of the video (by name on macOS, and by each of its windows asking on Windows). It then becomes
/// the one window.
///
/// # Errors
///
/// Returns an error if the window refused the request, or none was running and `view` could not be
/// started.
pub fn open_viewer(session: Option<&std::path::Path>) -> std::io::Result<()> {
    use crate::viewer::ipc;

    let request = ipc::Request {
        open: session.map(ipc::absolute).into_iter().collect(),
        ..ipc::Request::default()
    };
    if ipc::send(&ipc::default_socket(), &request)? == ipc::Delivery::Delivered {
        return Ok(());
    }

    // on Windows the window is the build without a console, so that no console window opens with it
    let this = std::env::current_exe()?;
    let gui = this.with_file_name(format!(
        "komorebi-recorder-gui{}",
        std::env::consts::EXE_SUFFIX
    ));
    let program = if cfg!(windows) && gui.is_file() {
        gui
    } else {
        this
    };
    let mut command = std::process::Command::new(program);
    // no console window flashing up for it
    #[cfg(windows)]
    crate::platform::process::isolate(&mut command);
    command.arg("view");
    if let Some(session) = session {
        command.arg(ipc::absolute(session));
    }
    command.spawn().map(|_| ())
}

/// Set by SIGINT/SIGTERM: the window finishes any recording in progress, then closes.
static QUIT: AtomicBool = AtomicBool::new(false);

/// Installs the handler that sets the quit flag on SIGINT and SIGTERM (it also runs on SIGHUP on
/// Unix, and on Ctrl-C on Windows), so that killing the window, or logging out, still finalizes a
/// recording in progress. Failing to install it (another is already installed) is ignored.
pub(crate) fn install_quit_handler() {
    let _ = ctrlc::set_handler(|| QUIT.store(true, Ordering::SeqCst));
}

/// How often the list of attached displays is refreshed while idle.
const DISPLAY_REFRESH: Duration = Duration::from_secs(1);

/// Opens the program's one window on the Record tab, in the small control window, and runs it
/// until it is closed.
///
/// `args` are the options of `komorebi-recorder record` that the Record tab starts from, such as
/// `--fps`, `--codec` and `--no-komorebi`. Each recording uses them, except that the displays ticked
/// in the tab replace `--display` (every display ticked means every display, including any
/// connected while recording) and a predicate typed under Advanced replaces `--predicate`.
///
/// `socket` is the path of the window's socket ([`crate::viewer::ipc::default_socket`]), or `None`
/// for a window of its own that no other process can reach (`gui --new-window`). With a socket, if
/// a window is already running it is asked to show its Record tab and this returns `Ok` at once
/// without making a window, and `args` are ignored: they only seed the window that is started.
///
/// This blocks the calling thread: the window's event loop runs on it. A recording that fails does
/// not make this fail: the tab shows why, and can record again. The window is described in
/// [`crate::viewer::run_with`]; whether it starts small comes from the preferences
/// ([`crate::viewer::prefs::Prefs::compact`]). Needs a window, so there is no example.
///
/// # Errors
///
/// Returns an error saying "the window failed" if the window cannot be created or its event loop
/// fails (for example, there is no display or no usable graphics adapter).
pub fn run(args: RecordArgs, socket: Option<&std::path::Path>) -> eyre::Result<()> {
    crate::viewer::run_with(
        crate::viewer::Launch {
            sessions: Vec::new(),
            record: args,
            show: crate::viewer::Show::Record,
        },
        socket,
    )
}

type Worker = JoinHandle<eyre::Result<PathBuf>>;

/// The recorder: what is needed to record, and the Record tab that shows it.
///
/// The host window makes one with [`RecorderApp::new`], calls [`RecorderApp::poll`] on every frame
/// and [`RecorderApp::ui`] on the frames the Record tab is showing. It owns the recording's thread,
/// so dropping it without stopping leaves that thread to finish on its own.
pub struct RecorderApp {
    args: RecordArgs,
    live: Arc<Live>,
    worker: Option<Worker>,
    permission: bool,
    displays: Vec<(DisplayDescriptor, bool)>,
    displays_refreshed: Instant,
    last: Option<Result<PathBuf, String>>,
    closing: bool,
    predicate: String,
    show_streams: bool,
    quit: &'static AtomicBool,
}

/// What the Record tab asks of the window in one frame.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecorderOutput {
    /// "Open session…" was pressed: ask for a folder and open it in a tab.
    pub choose: bool,
    /// "Open last" was pressed: open this session in a tab.
    pub open: Option<PathBuf>,
    /// "Compact" or "Expand" was pressed.
    pub toggle_compact: bool,
}

impl RecorderApp {
    /// Returns a recorder that starts from `args`: see [`run`] for how they are used. It reads the
    /// attached displays and whether the screen may be recorded, and records nothing.
    #[must_use]
    pub fn new(args: RecordArgs) -> Self {
        let mut app = Self {
            predicate: args.predicate.clone().unwrap_or_default(),
            args,
            live: Live::new(),
            worker: None,
            permission: capture::has_permission(),
            displays: Vec::new(),
            displays_refreshed: Instant::now() - DISPLAY_REFRESH,
            last: None,
            closing: false,
            show_streams: false,
            quit: &QUIT,
        };

        app.refresh_displays();
        app
    }

    /// Re-read the attached displays (cheap, and needs no permission), keeping the ticks of the
    /// ones that are still there. Displays that were docked or undocked since the last look
    /// appear and disappear here, and a new one starts ticked.
    fn refresh_displays(&mut self) {
        self.displays_refreshed = Instant::now();
        let Ok(present) = capture::active_displays() else {
            return;
        };

        let previous: Vec<(u32, bool)> = self
            .displays
            .iter()
            .map(|(display, selected)| (display.id, *selected))
            .collect();

        self.displays = present
            .into_iter()
            .map(|display| {
                let selected = previous
                    .iter()
                    .find(|(id, _)| *id == display.id)
                    .is_none_or(|(_, selected)| *selected);
                (display, selected)
            })
            .collect();
    }

    /// Whether a recording is going: from pressing Record until its thread has been joined,
    /// including while it finalizes.
    #[must_use]
    pub fn recording(&self) -> bool {
        self.worker.is_some()
    }

    /// Says `note` in the tab as a message (as an error, in red), for something the window learned
    /// before it was shown.
    pub fn say(&mut self, note: String) {
        self.last = Some(Err(note));
    }

    /// Returns the session the last recording saved, or `None` if none has finished since the
    /// window was started, or one is going.
    #[must_use]
    pub fn saved(&self) -> Option<&std::path::Path> {
        match &self.last {
            Some(Ok(dir)) => Some(dir),
            _ => None,
        }
    }

    /// Returns the text of the Record tab in the tab bar, and whether it is marking a recording
    /// (so the window can draw it in red): `Record` when idle, `● REC 00:12` while recording.
    #[must_use]
    pub fn tab_label(&self) -> (String, bool) {
        let snapshot = self.live.snapshot();
        if !self.recording() {
            return ("Record".to_string(), false);
        }
        let text = match snapshot.phase {
            Phase::Starting => "● starting…".to_string(),
            Phase::Stopping => "● finalizing…".to_string(),
            _ => format!("● REC {}", timecode::format(snapshot.elapsed_s)),
        };
        (text, true)
    }

    fn start(&mut self) {
        let mut args = self.args.clone();
        // with every display ticked, record *every* display including ones connected while
        // recording; otherwise only the ticked ones
        args.displays = if self.displays.iter().all(|(_, selected)| *selected) {
            Vec::new()
        } else {
            self.displays
                .iter()
                .filter(|(_, selected)| *selected)
                .map(|(display, _)| display.id)
                .collect()
        };
        let predicate = self.predicate.trim();
        args.predicate = (!predicate.is_empty()).then(|| predicate.to_string());

        let options = args.to_options();
        self.live.reset();
        self.last = None;

        let live = self.live.clone();
        match std::thread::Builder::new()
            .name("recorder".to_string())
            .spawn(move || recorder::record(&options, &live))
        {
            Ok(worker) => self.worker = Some(worker),
            Err(error) => self.last = Some(Err(format!("could not start recording: {error}"))),
        }
    }

    /// Does what the recorder has to do on every frame, whichever tab is showing: refreshes the
    /// displays while idle, joins a recording that has finished (its session becomes
    /// [`RecorderApp::saved`], or the reason it failed is shown), and honours a close request or
    /// SIGINT/SIGTERM. While recording, those cancel the close, ask the recording to stop and close
    /// the window once it has finalized; with none going they close it. It also asks `context` to
    /// draw again soon, so the label of the tab keeps time.
    pub fn poll(&mut self, context: &egui::Context) {
        context.request_repaint_after(if self.recording() {
            Duration::from_millis(200)
        } else {
            Duration::from_millis(500)
        });
        if !self.recording() && self.displays_refreshed.elapsed() >= DISPLAY_REFRESH {
            self.refresh_displays();
        }

        if self.worker.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(worker) = self.worker.take()
        {
            self.last = Some(match worker.join() {
                Ok(Ok(dir)) => Ok(dir),
                Ok(Err(error)) => Err(format!("{error:#}")),
                Err(_) => Err("the recording thread panicked".to_string()),
            });
            self.live.reset();
        }

        let quit_signalled = self.quit.load(Ordering::SeqCst);
        if (quit_signalled || context.input(|input| input.viewport().close_requested()))
            && self.recording()
        {
            // finish and index the session before exiting rather than leaving it half-written
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.live.request_stop();
            self.closing = true;
        }

        if (self.closing || quit_signalled) && !self.recording() {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Draws the Record tab into `ui`: the content of the tab, with no window chrome of its own.
    ///
    /// `compact` says whether the window is the small control window, which changes the label of
    /// the button that switches it. It returns what the tab asked the window to do.
    pub fn ui(&mut self, ui: &mut egui::Ui, compact: bool) -> RecorderOutput {
        let mut output = RecorderOutput::default();
        let snapshot = self.live.snapshot();
        let recording = self.recording();
        let busy = matches!(snapshot.phase, Phase::Starting | Phase::Stopping);

        egui::ScrollArea::vertical()
            .id_salt("record-tab")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("komorebi-recorder").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, tip) = if compact {
                            ("Expand", "the full window, with tabs for sessions")
                        } else {
                            ("Compact", "the small window that stays on top")
                        };
                        if ui.button(label).on_hover_text(tip).clicked() {
                            output.toggle_compact = true;
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let can_record = !recording && self.permission && !self.displays.is_empty();
                    if ui
                        .add_enabled(
                            can_record,
                            egui::Button::new("Record").min_size([110.0, 32.0].into()),
                        )
                        .clicked()
                    {
                        self.start();
                    }

                    if ui
                        .add_enabled(
                            recording && !busy && !self.live.stop_requested(),
                            egui::Button::new("Stop").min_size([110.0, 32.0].into()),
                        )
                        .clicked()
                    {
                        self.live.request_stop();
                    }

                    let state = match snapshot.phase {
                        Phase::Starting => "starting...".to_string(),
                        Phase::Recording => format!("REC {}", timecode::format(snapshot.elapsed_s)),
                        Phase::Stopping => "finalizing...".to_string(),
                        _ => "ready".to_string(),
                    };
                    ui.label(egui::RichText::new(state).size(18.0).monospace());
                });

                ui.separator();

                // reviewing: open a recording in the viewer, which is its own window
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Review").small().weak());
                    if ui
                        .button("Open session…")
                        .on_hover_text("play and scrub a recording")
                        .clicked()
                        && let Err(error) = open_viewer(None)
                    {
                        self.last = Some(Err(format!("could not open the viewer: {error}")));
                    }

                    let last = match &self.last {
                        Some(Ok(dir)) => Some(dir.clone()),
                        _ => latest_session_in(&crate::session::default_recordings_dir()),
                    };
                    if ui
                        .add_enabled(last.is_some(), egui::Button::new("Open last"))
                        .on_hover_text("the most recent recording")
                        .clicked()
                        && let Some(dir) = last
                        && let Err(error) = open_viewer(Some(&dir))
                    {
                        self.last = Some(Err(format!("could not open the viewer: {error}")));
                    }
                });

                ui.separator();

                if !self.permission {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "Screen Recording permission is needed to record the screen.",
                    );
                    if ui.button("Grant permission").clicked() {
                        match capture::ensure_permission() {
                            Ok(()) => self.permission = true,
                            Err(error) => self.last = Some(Err(format!("{error:#}"))),
                        }
                    }
                } else {
                    ui.add_enabled_ui(!recording, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Displays:");
                            for (display, selected) in &mut self.displays {
                                ui.checkbox(
                                    selected,
                                    format!(
                                        "{} ({}×{})",
                                        display.id, display.width_points, display.height_points
                                    ),
                                );
                            }
                        });
                    });
                }

                if recording {
                    ui.add_space(6.0);
                    egui::Grid::new("counters").num_columns(2).show(ui, |ui| {
                        ui.label("displays");
                        ui.label(if snapshot.displays_retrying.is_empty() {
                            format!("{} recording", snapshot.displays_recording)
                        } else {
                            format!(
                                "{} recording, waiting to retry {:?}",
                                snapshot.displays_recording, snapshot.displays_retrying
                            )
                        });
                        ui.end_row();
                        ui.label("video frames");
                        ui.label(format!(
                            "{} ({} dropped)",
                            snapshot.frames, snapshot.dropped
                        ));
                        ui.end_row();
                        ui.label("komorebi log");
                        ui.label(human_bytes(snapshot.log_bytes));
                        ui.end_row();
                        ui.label("komorebi events");
                        ui.label(snapshot.events.to_string());
                        ui.end_row();
                        ui.label("OS trace");
                        ui.label(format!(
                            "{} events, {} across {} streams",
                            snapshot.oslog_events,
                            human_bytes(snapshot.oslog_bytes),
                            snapshot.streams.len()
                        ));
                        ui.end_row();
                    });

                    ui.collapsing("OS trace streams", |ui| {
                        self.show_streams = true;
                        for stream in &snapshot.streams {
                            ui.label(format!(
                                "{}: {} events, {}",
                                stream.name,
                                stream.events,
                                human_bytes(stream.bytes)
                            ));
                        }
                    });
                } else {
                    ui.collapsing("Advanced", |ui| {
                        ui.label("`system` OS trace predicate (blank for the default):");
                        ui.text_edit_singleline(&mut self.predicate);
                    });
                }

                if !snapshot.incidents.is_empty() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Recent events").small().weak());
                    for incident in snapshot.incidents.iter().rev().take(4) {
                        ui.label(egui::RichText::new(incident).small().weak());
                    }
                }

                if !snapshot.warnings.is_empty() {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .max_height(110.0)
                        .id_salt("warnings")
                        .show(ui, |ui| {
                            for warning in &snapshot.warnings {
                                ui.colored_label(egui::Color32::YELLOW, format!("! {warning}"));
                            }
                        });
                }

                match &self.last {
                    Some(Ok(dir)) => {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.label("Saved session:");
                        ui.monospace(dir.display().to_string());
                        ui.horizontal(|ui| {
                            if ui.button("Copy path").clicked() {
                                ui.ctx().copy_text(dir.display().to_string());
                            }

                            if ui
                                .button(format!("Open in {}", shell::FILE_MANAGER))
                                .clicked()
                            {
                                shell::reveal(dir);
                            }
                        });
                        ui.label(format!(
                            "Investigate it: komorebi-recorder brief {}",
                            dir.display()
                        ));
                    }
                    Some(Err(error)) => {
                        ui.add_space(8.0);
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                    None => {}
                }
            });
        output
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
impl RecorderApp {
    /// A recorder that touches no screen: no permission check, no displays, and its own quit flag,
    /// for the tests of the window.
    pub(crate) fn quiet(quit: &'static AtomicBool) -> Self {
        Self {
            args: RecordArgs::default(),
            live: Live::new(),
            worker: None,
            permission: true,
            displays: Vec::new(),
            displays_refreshed: Instant::now(),
            last: None,
            closing: false,
            predicate: String::new(),
            show_streams: false,
            quit,
        }
    }

    /// Makes the recorder look as if `worker` were a recording that has begun, with `phase`.
    pub(crate) fn pretend(&mut self, worker: Worker, phase: Phase, elapsed_s: f64) {
        self.live.update(|snapshot| {
            snapshot.phase = phase;
            snapshot.elapsed_s = elapsed_s;
        });
        self.worker = Some(worker);
    }

    /// The handle the recording would publish through.
    pub(crate) fn live(&self) -> Arc<Live> {
        self.live.clone()
    }

    /// Whether the recorder has been asked to close the window once the recording is done.
    pub(crate) fn closing(&self) -> bool {
        self.closing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "komorebi-recorder-app-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn the_latest_session_is_the_last_by_name_and_must_be_a_session() {
        let recordings = temp("recordings");
        for name in ["20261026T100000Z", "20261026T120000Z", "20261025T090000Z"] {
            let dir = recordings.join(name);
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(dir.join(crate::session::MANIFEST_FILE), "{}").expect("manifest");
        }
        // a newer folder that is not a session does not count
        std::fs::create_dir_all(recordings.join("20261027T000000Z")).expect("dir");

        assert_eq!(
            latest_session_in(&recordings),
            Some(recordings.join("20261026T120000Z"))
        );
        assert_eq!(latest_session_in(&recordings.join("nothing-here")), None);
    }
}
