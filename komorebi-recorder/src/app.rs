//! The recorder window: Record / Stop, live counters and warnings.
//!
//! This is what `komorebi-recorder` opens when it is run with no subcommand, and with `gui`: [`run`]
//! shows a small window and keeps it until it is closed. It is the same recorder as
//! `komorebi-recorder record`, with a window instead of Ctrl-C.
//!
//! # What the window does
//!
//! | control | effect |
//! |---|---|
//! | Record | starts a recording on a thread named `recorder` ([`recorder::record`]), with the options the window was started with, the displays ticked below and the predicate typed under Advanced |
//! | Stop | asks the recording to finish ([`Live::request_stop`]); it indexes and finalizes the session, which the window shows as "finalizing..." |
//! | Open session..., Open last | open the viewer on a recording, in a process of its own |
//! | Copy path, Open in the file manager | after a recording: copy where the session was saved, or show it (Finder or Explorer) |
//!
//! While idle the list of displays is read again every second, so a display that was docked or
//! undocked appears or disappears and a new one starts ticked. With every display ticked the
//! recorder follows all of them, including any connected while recording; with one unticked it
//! records only the ticked ones. While recording the window shows counters (video frames and the
//! frames dropped, the size of komorebi's log, the number of komorebi events, the OS trace and each
//! of its streams), the most recent incidents to the recording environment, and any warnings.
//!
//! The window does not mark where a bug is: that is done afterwards, in the viewer.
//!
//! Closing the window while recording, or delivering SIGINT or SIGTERM to the process (the handler
//! also runs on SIGHUP on Unix, and on Ctrl-C on Windows), finishes and indexes the session first
//! rather than leaving it half-written, and only then closes the window.
//!
//! # Staying out of the video and out of the layout
//!
//! The window is for use on the machine whose behavior is being recorded, so it must not appear in
//! the video and must not be tiled by komorebi.
//!
//! - **Not in the video.** On macOS the capture filter excludes this whole application (this
//!   process and every process named `komorebi-recorder`), so the controls never appear in the
//!   video. On Windows a whole monitor is captured, so about once a second the window asks Windows
//!   to leave it out of captures (`WDA_EXCLUDEFROMCAPTURE`) through
//!   [`crate::platform::window::Chrome`].
//! - **Not tiled.** The window is deliberately borderless and untitled, with its own drag handle
//!   and close button, and always on top. komorebi does not manage windows that have no title (komorebi
//!   for Mac rejects them in `Window::should_manage`), so opening the recorder does not perturb the
//!   very layout being observed: a titled window would be tiled into the workspace. On Windows
//!   `KOMOREBI_RECORDER_PROBE_WINDOW=1` makes the window ask komorebi whether it would manage it
//!   (see [`crate::platform::window::PROBE_VARIABLE`]).
//!
//! The window needs an `egui::Context` to draw and a screen to record, so it has no example. Its
//! logic that does not need either is covered by this module's unit test
//! (`the_latest_session_is_the_last_by_name_and_must_be_a_session`).

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

/// Open the viewer, in its own process, on `session` if given. A separate process, so that
/// reviewing a recording can never disturb one being made, and kept out of the video (by name on
/// macOS, and by each of its windows asking on Windows).
fn open_viewer(session: Option<&std::path::Path>) -> std::io::Result<()> {
    // on Windows the viewer is the build without a console, so that no console window opens with it
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
        command.arg(session);
    }
    command.spawn().map(|_| ())
}

/// Set by SIGINT/SIGTERM: the window finishes any recording in progress, then closes.
static QUIT: AtomicBool = AtomicBool::new(false);

/// How often the list of attached displays is refreshed while idle.
const DISPLAY_REFRESH: Duration = Duration::from_secs(1);

/// Opens the recorder window and runs it until it is closed.
///
/// `args` are the options of `komorebi-recorder record` that the window starts from, such as
/// `--fps`, `--codec` and `--no-komorebi`. Each recording uses them, except that the displays ticked in
/// the window replace `--display` (every display ticked means every display, including any
/// connected while recording) and a predicate typed under Advanced replaces `--predicate`.
///
/// This blocks the calling thread: the window's event loop runs on it. It installs a handler for
/// SIGINT and SIGTERM (it also runs on SIGHUP on Unix, and on Ctrl-C on Windows) that asks the
/// window to finish any recording in progress and close, so that killing the window, or logging
/// out, still finalizes the session. Failing to install the handler (another is already
/// installed) is ignored.
///
/// A recording that fails does not make this fail: the window shows why, and can record again.
///
/// The window is 420 by 380 points (at least 360 by 260), borderless, untitled and always on top:
/// see the [module documentation](self#staying-out-of-the-video-and-out-of-the-layout). Needs a
/// window, so there is no example.
///
/// # Errors
///
/// Returns an error saying "the recorder window failed" if the window cannot be created or its
/// event loop fails (for example, there is no display or no usable graphics adapter).
pub fn run(args: RecordArgs) -> eyre::Result<()> {
    // killing the window (or logging out) must still finalize a recording in progress
    let _ = ctrlc::set_handler(|| QUIT.store(true, Ordering::SeqCst));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // no title: komorebi leaves untitled windows alone
            .with_title("")
            .with_decorations(false)
            .with_inner_size([420.0, 380.0])
            .with_min_inner_size([360.0, 260.0])
            .with_always_on_top(),
        ..Default::default()
    };

    eframe::run_native(
        "komorebi-recorder",
        options,
        Box::new(|_creation_context| Ok(Box::new(RecorderApp::new(args)))),
    )
    .map_err(|error| eyre::Report::msg(format!("the recorder window failed: {error}")))
}

type Worker = JoinHandle<eyre::Result<PathBuf>>;

struct RecorderApp {
    chrome: crate::platform::window::Chrome,
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
}

impl RecorderApp {
    fn new(args: RecordArgs) -> Self {
        let mut app = Self {
            chrome: crate::platform::window::Chrome::default(),
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

    fn recording(&self) -> bool {
        self.worker.is_some()
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

    fn poll(&mut self, context: &egui::Context) {
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

        let quit_signalled = QUIT.load(Ordering::SeqCst);
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

impl eframe::App for RecorderApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.chrome.tick();
        self.poll(context);
        context.request_repaint_after(Duration::from_millis(200));

        let snapshot = self.live.snapshot();
        let recording = self.recording();
        let busy = matches!(snapshot.phase, Phase::Starting | Phase::Stopping);

        egui::CentralPanel::default().show(context, |ui| {
            // drag handle and close button, since there is no title bar
            let header = egui::Rect::from_min_size(
                ui.min_rect().min,
                egui::vec2(ui.available_width(), 24.0),
            );
            let drag = ui.interact(header, ui.id().with("drag"), egui::Sense::drag());
            if drag.drag_started() {
                context.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("komorebi-recorder").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("×").on_hover_text("Close").clicked() {
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
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
                            context.copy_text(dir.display().to_string());
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
