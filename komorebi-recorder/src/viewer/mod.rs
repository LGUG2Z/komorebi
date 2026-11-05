//! The program's one window: recording and reviewing, in tabs.
//!
//! `komorebi-recorder` with no command, `komorebi-recorder gui` and `komorebi-recorder view
//! [SESSION...]` ([`crate::cli::GuiArgs`], [`crate::cli::ViewArgs`]) all open this window, with
//! [`run_with`] (the recorder's [`crate::app::run`] and the viewer's [`run`] are the two ways in),
//! or hand their request to the one that is already open. It is an `egui` application run by
//! `eframe`.
//!
//! The first tab is **Record**: the recorder ([`crate::app`]) with its Record and Stop buttons. It
//! is pinned, cannot be closed, and shows `REC` and the time in the tab bar while a recording goes
//! on, which it does whichever tab is showing. After it come the session tabs, one for each
//! recording being reviewed. There is only ever one window: opening another session (from the
//! window, from the command line or from the Record tab) adds a tab to the window that is already
//! open instead of making a new one: see [`ipc`] and [Tabs and one window](self#tabs-and-one-window).
//!
//! ```text
//! +-------------------------------------------------+
//! | [Record] [15:46:19 x] [15:51:02 x] [+]  Open... |  tab bar
//! | session name  counts and warnings               |  top bar (a session)
//! +----------------------------------+--------------+
//! |                                  | Now          |
//! |   the video of every display,    | Event        |
//! |   side by side, with komorebi's  | Findings     |  inspector
//! |   layout drawn over it           | Bugs         |
//! |                                  | Log          |
//! +----------------------------------+--------------+
//! | transport controls, then the lanes: ruler, bugs,|
//! | events, log, machine, findings, screen, video   |  timeline
//! +-------------------------------------------------+
//! ```
//!
//! # Compact and full
//!
//! Started plainly (`komorebi-recorder`, `gui`), the window is *compact*: the old small recorder
//! window, 420 by 380 points (at least 360 by 260), always on top, showing the Record tab alone
//! with no tab bar, to leave over the application a bug is being reproduced in. The Expand button
//! of the Record tab, and opening any session (the Review buttons, a request from another process,
//! a key), make it the full window: 1500 by 920 points (at least 900 by 600), not on top, with the
//! tab bar; the size it had last is restored. The Compact button goes back, and the choice is
//! kept in the preferences ([`prefs::Prefs::compact`]). `view` with sessions starts full. While
//! recording, the full window has a "pin on top" checkbox. The window uses the system's title bar
//! and frame on Windows; on macOS the title bar is hidden and the content runs under it.
//!
//! # Tabs and one window
//!
//! A session tab is one session ([`Open`]), or a start screen with the recent sessions, or a
//! session that failed to open with the reason. A tab is named for when its session began
//! (`15:46:19`, from the folder's name), with the whole path on hover; `x` (or a middle click)
//! closes it and `+` adds a start screen. The Record tab has no `x`. Closing the last session tab
//! shows the Record tab; the window itself is closed with the system's close button. Opening a
//! session follows one rule, whatever asked for it: if a tab has the session, that tab is shown;
//! else if the current tab is a start screen or a failure the session loads into it; else it goes
//! in a new tab. Only the tab in view plays: the others are paused (their scan for changes carries
//! on), and closing one stops its scan and its decoding. The keys are in [`keymap`] (`Cmd`/`Ctrl`
//! `T`, `W`, `Tab`, `0` for the Record tab, and `1` to `9`). Tabs are not restored when the window
//! is started again; the recent sessions are shared by all of them.
//!
//! Every launch that opens a window finds out whether one is already running by connecting to a
//! socket in komorebi's data directory, `komorebi-recorder-viewer.sock`. If one answers, it is
//! sent what was asked (the sessions as absolute paths, or the Record tab for `gui`) and asked to
//! come to the front, and the command exits without a window; if not, the command becomes the
//! window and listens there. A second `gui` therefore raises the window and shows its Record tab,
//! and its options (`--fps` and the rest) are ignored: they only seed the window that is started.
//! `--new-window` on `gui` and `view` skips the socket and makes an independent window.
//! `komorebi-recorder-gui.exe` (the Windows build with no console) uses the same socket. The
//! protocol, the start-up race and a stale socket are covered in [`ipc`].
//!
//! Two environment variables point the window somewhere else, for trying it out or testing it end
//! to end without touching the real ones: `KOMOREBI_RECORDER_VIEWER_SOCKET` names another socket
//! ([`ipc::SOCKET_VARIABLE`]) and `KOMOREBI_RECORDER_PREFS_DIR` another directory for the
//! preferences ([`prefs::DIRECTORY_VARIABLE`]).
//!
//! # What plays and what is read
//!
//! The viewer reads a *finished* session; one that is still being recorded does not open. A
//! session opens in the background ([`loaded::Loaded::load`]) while the window shows a spinner, and
//! a session that cannot be opened is shown on the start screen with the reason. Playing is
//! done by [`Playback`](crate::playback::Playback), whose worker thread decodes the video; the
//! window only asks it for the frame to show. Where the picture changed is found by a scan on yet
//! another thread after the session opens.
//!
//! The session may have been recorded on either host and by either flavor of komorebi: the viewer
//! shows whatever [`Analysis`](crate::analysis::Analysis) reads from it, and the video plays on
//! any host that has a decoder (macOS and Windows).
//!
//! # Modules
//!
//! | module | what it is |
//! |---|---|
//! | [`ipc`] | the window's local socket: how one window takes requests from other processes |
//! | [`keymap`] | the keys: a key press becomes an [`Action`](keymap::Action) |
//! | [`timeline`] | the arithmetic of the timeline: the visible stretch, the ruler, dense lanes |
//! | [`loaded`] | the session as the window draws it, read once |
//! | [`prefs`] | what the window remembers between runs |
//! | [`theme`] | colors |
//! | `app` | [`ViewerApp`] (the Record tab and the session tabs), [`Screen`] (what a session tab shows) and [`Open`], the state of the window and how it is kept in step |
//! | `lanes` | the transport bar and the timeline lanes, and what a mouse does on them |
//! | `inspector` | the panel on the right and its five tabs |
//! | `video` | the pictures and the state overlay drawn over them |
//! | `composer` | the form for marking a bug region, and editing, deleting and bringing one back |
//!
//! The first six are public, because they are plain values and arithmetic that need no window. The
//! rest draw, so they need an `egui::Context`: their items are documented, and there are no
//! examples for them. They are covered by the tests in `viewer::app`, which drive the whole window
//! headlessly with real key events.
//!
//! # Staying out of the way of what is being investigated
//!
//! The window is used on the machine whose behavior is being investigated, so it must neither be
//! tiled by komorebi nor appear in a recording being made, in either size.
//!
//! - **Not tiled.** The window is named `komorebi-recorder` only when it is safely ignored
//!   ([`crate::komorebi_ignore`]): [`run_with`] first registers `komorebic ignore-rule exe` for the
//!   recorder's executables with a running komorebi for Windows, or finds none running. When it
//!   cannot (no `komorebic`, the command fails, or any macOS host, where komorebi manages every
//!   window with a title) the title is empty, and komorebi leaves windows with no title alone.
//!   The three settings that also hide the title bar apply on macOS only. Whether
//!   komorebi for Windows leaves the window alone is checked on a real machine rather than assumed:
//!   with `KOMOREBI_RECORDER_PROBE_WINDOW=1` set, the window asks komorebi whether it would manage
//!   it and writes the answer to `komorebi-recorder-window-probe.txt` in the temp directory
//!   (see [`crate::platform::window::PROBE_VARIABLE`]).
//! - **Not in the video.** On macOS the recorder's capture filter excludes by name every process
//!   called `komorebi-recorder` (and its own), which includes this window. On Windows a whole
//!   monitor is captured, so about once a second the window asks Windows to leave it out of captures
//!   (`WDA_EXCLUDEFROMCAPTURE`) through [`crate::platform::window::Chrome`], which it ticks on
//!   every frame.

mod app;
mod composer;
mod inspector;
pub mod ipc;
pub mod keymap;
mod lanes;
pub mod loaded;
pub mod prefs;
mod rules;
pub mod theme;
pub mod timeline;
mod video;

use color_eyre::eyre;
use eframe::egui;
use std::path::Path;
use std::path::PathBuf;

pub use app::Open;
pub use app::Screen;
pub use app::ViewerApp;

/// What the window is started to show: the Record tab (plain `gui`) or sessions (`view`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    /// The Record tab, in the compact window if the preferences say so.
    Record,
    /// The sessions in [`Launch::sessions`], in the full window; with none, a start screen.
    Sessions,
}

/// What a launch of the window asks for.
#[derive(Debug, Clone)]
pub struct Launch {
    /// The sessions to open as tabs, as given. Only used with [`Show::Sessions`].
    pub sessions: Vec<PathBuf>,
    /// The options the Record tab starts from (see [`crate::app::run`]).
    pub record: crate::cli::RecordArgs,
    /// What to show.
    pub show: Show,
}

impl Launch {
    /// Returns the launch of plain `gui`: the Record tab, starting from `record`.
    #[must_use]
    pub fn record(record: crate::cli::RecordArgs) -> Self {
        Self {
            sessions: Vec::new(),
            record,
            show: Show::Record,
        }
    }

    /// Returns the launch of `view`: `sessions` as tabs, in the full window.
    #[must_use]
    pub fn sessions(sessions: Vec<PathBuf>) -> Self {
        Self {
            sessions,
            record: crate::cli::RecordArgs::default(),
            show: Show::Sessions,
        }
    }

    /// Whether the window starts as the small control window: when it shows the Record tab and
    /// `prefs` say so.
    #[must_use]
    pub fn starts_compact(&self, prefs: &prefs::Prefs) -> bool {
        self.show == Show::Record && prefs.compact
    }

    /// The request that hands this launch to a window that is already running.
    fn request(&self) -> ipc::Request {
        match self.show {
            Show::Record => ipc::Request {
                tab: Some(ipc::TabName::Record),
                ..ipc::Request::default()
            },
            Show::Sessions => ipc::Request {
                open: self
                    .sessions
                    .iter()
                    .map(|path| ipc::absolute(path))
                    .collect(),
                ..ipc::Request::default()
            },
        }
    }
}

/// Opens the window with each of `sessions` open in a tab, and runs it until it is closed: what
/// `komorebi-recorder view` does. See [`run_with`], of which this is the launch [`Launch::sessions`].
///
/// # Errors
///
/// As [`run_with`].
pub fn run(sessions: Vec<PathBuf>, socket: Option<&Path>) -> eyre::Result<()> {
    run_with(Launch::sessions(sessions), socket)
}

/// Opens the window as `launch` asks, and runs it until it is closed.
///
/// This blocks the calling thread: the window's event loop runs on it. Each of the sessions is the
/// folder of a recording. They open in the background, so a session that cannot be opened does not
/// make this fail: the window comes up and shows why. With [`Show::Sessions`] and no sessions the
/// window shows a start screen with the recent sessions that still exist
/// ([`prefs::Prefs::existing`]) and a button to choose one.
///
/// `socket` is the path of the window's socket ([`ipc::default_socket`]), or `None` for a window of
/// its own that no other process can reach (`--new-window`). With a socket, if a window is already
/// running it is handed the launch (the sessions as tabs, or its Record tab, which raises it) and
/// this returns `Ok` at once without making a window; the options of [`Launch::record`] are then
/// ignored. Otherwise this window listens on the socket and takes what arrives as tabs. If the
/// socket cannot be used, a message is printed and the window is made without it.
///
/// The window is 1500 by 920 points (at least 900 by 600), or 420 by 380 (at least 360 by 260) and
/// always on top when compact ([`Launch::starts_compact`]), dark, and titled only when komorebi
/// ignores it: see the [module documentation](self#staying-out-of-the-way-of-what-is-being-investigated)
/// for why. A SIGINT or SIGTERM while recording finishes the recording and then closes the window.
/// Needs a window, so there is no example; the tests in `viewer::app` drive the same [`ViewerApp`]
/// headlessly.
///
/// # Errors
///
/// Returns an error saying "the window failed" if the window cannot be created or its event loop
/// fails (for example, there is no display or no usable graphics adapter).
pub fn run_with(launch: Launch, socket: Option<&Path>) -> eyre::Result<()> {
    let bound = match socket {
        Some(socket) => match ipc::claim(socket, &launch.request()) {
            // a window is running and has taken the request
            Ok(ipc::Claim::Forwarded) => return Ok(()),
            Ok(ipc::Claim::Serving(bound)) => Some(bound),
            // not being able to share a window is no reason not to show the recording
            Err(error) => {
                eprintln!("could not reach or start the window's socket: {error}");
                None
            }
        },
        None => None,
    };

    // killing the window (or logging out) must still finalize a recording in progress
    crate::app::install_quit_handler();

    // only a window that is really going to open needs komorebi to ignore it, and it must be told
    // before the window exists, so komorebi never sees a titled window it was not told to ignore
    let setup = crate::komorebi_ignore::prepare_window();

    let directory = prefs::directory();
    let compact = launch.starts_compact(&prefs::Prefs::load(&directory));
    let (size, min) = if compact {
        (app::COMPACT_SIZE, app::COMPACT_MIN)
    } else {
        (app::FULL_SIZE, app::FULL_MIN)
    };
    let mut viewport = egui::ViewportBuilder::default()
        // titled only when komorebi ignores it (or is not running), so opening the window does
        // not move the layout of the machine whose behavior is being investigated
        .with_title(setup.title)
        .with_app_id("komorebi-recorder")
        .with_icon(crate::icon::window_icon())
        .with_title_shown(false)
        .with_titlebar_shown(false)
        .with_fullsize_content_view(true)
        .with_inner_size(size)
        .with_min_inner_size(min);
    if compact {
        // the small control window stays over the application being recorded
        viewport = viewport.with_always_on_top();
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "komorebi-recorder",
        options,
        Box::new(move |creation| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = theme::BACKGROUND;
            visuals.window_fill = theme::BACKGROUND;
            creation.egui_ctx.set_visuals(visuals);

            let mut app = ViewerApp::start(directory, launch);
            if let Some(note) = setup.note {
                app.recorder_mut().say(note);
            }
            if let Some(bound) = bound {
                let ctx = creation.egui_ctx.clone();
                app.listen(bound.serve(move || ctx.request_repaint()));
            }
            Ok(Box::new(Shell(
                app,
                crate::platform::window::Chrome::default(),
            )))
        }),
    )
    .map_err(|error| eyre::Report::msg(format!("the window failed: {error}")))
}

/// Adapts [`ViewerApp`] to eframe.
struct Shell(ViewerApp, crate::platform::window::Chrome);

impl eframe::App for Shell {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        // keeps the window out of any recording being made (on Windows) and, if asked, finds out
        // whether komorebi would tile it
        self.1.tick();
        self.0.ui(context);
    }
}
