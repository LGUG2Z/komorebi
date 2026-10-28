//! The viewer: a window for playing and scrubbing a recording with everything that happened in it
//! laid out on a timeline.
//!
//! It is started by `komorebi-recorder view [SESSION...]` ([`crate::cli::ViewArgs`]), which calls
//! [`run`], and by the "Open session..." and "Open last" buttons of the recorder window
//! ([`crate::app`]), which start that command in a process of its own so that reviewing a
//! recording cannot disturb one being made. It is an `egui` application run by `eframe`.
//!
//! There is one viewer window. Each session is a tab of it, and opening another session (from
//! the window, from the command line or from the recorder window) adds a tab to the window that
//! is already open instead of making a new one: see [`ipc`] and
//! [Tabs and one window](self#tabs-and-one-window).
//!
//! ```text
//! +-------------------------------------------------+
//! | [15:46:19 x] [15:51:02 x] [+]  Open...          |  tab bar
//! | session name  counts and warnings               |  top bar
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
//! # Tabs and one window
//!
//! A tab is one session ([`Open`]), or a start screen with the recent sessions, or a session that
//! failed to open with the reason. A tab is named for when its session began (`15:46:19`, from the
//! folder's name), with the whole path on hover; `x` (or a middle click) closes it and `+` adds a
//! start screen. Closing the last tab leaves a start screen: the window itself is closed
//! with the system's close button. Opening a session follows one rule, whatever asked for it: if a
//! tab has the session, that tab is shown; else if the current tab is a start screen or a failure
//! the session loads into it; else it goes in a new tab. Only the tab in view plays: the others
//! are paused (their scan for changes carries on), and closing one stops its scan and its decoding.
//! The keys are in [`keymap`] (`Cmd`/`Ctrl` `T`, `W`, `Tab`, and `1` to `9`). Tabs are not
//! restored when the viewer is started again; the recent sessions are shared by all of them.
//!
//! `komorebi-recorder view` finds out whether a viewer is running by connecting to a socket in
//! komorebi's data directory, `komorebi-recorder-viewer.sock`. If one answers, it is sent the
//! sessions (as absolute paths) and asked to come to the front, and the command exits without a
//! window; if not, the command becomes the viewer and listens there. `view --new-window` skips
//! the socket and makes an independent window. The recorder window talks to the socket directly and
//! starts `view` only if nobody answers. The protocol, the start-up race and a stale socket
//! are covered in [`ipc`]. `KOMOREBI_RECORDER_VIEWER_SOCKET` names another socket, for trying the
//! viewer out without touching a real one.
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
//! | [`ipc`] | the viewer's local socket: how one window takes sessions from other processes |
//! | [`keymap`] | the keys: a key press becomes an [`Action`](keymap::Action) |
//! | [`timeline`] | the arithmetic of the timeline: the visible stretch, the ruler, dense lanes |
//! | [`loaded`] | the session as the window draws it, read once |
//! | [`prefs`] | what the viewer remembers between runs |
//! | [`theme`] | colors |
//! | `app` | [`ViewerApp`] (the tabs), [`Screen`] (what a tab shows) and [`Open`], the state of the window and how it is kept in step |
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
//! The viewer is used on the machine whose behavior is being investigated, so it must neither be
//! tiled by komorebi nor appear in a recording being made.
//!
//! - **Not tiled.** The window is named `komorebi-recorder` only when it is safely ignored
//!   ([`crate::komorebi_ignore`]): [`run`] first registers `komorebic ignore-rule exe` for the
//!   recorder's executables with a running komorebi for Windows, or finds none running. When it
//!   cannot (no `komorebic`, the command fails, or any macOS host, where komorebi manages every
//!   window with a title) the title is empty, and komorebi leaves windows with no title alone.
//!   The three settings that also hide the title bar apply on macOS only. Whether
//!   komorebi for Windows leaves the window alone is checked on a real machine rather than assumed:
//!   with `KOMOREBI_RECORDER_PROBE_WINDOW=1` set, the window asks komorebi whether it would manage
//!   it and writes the answer to `komorebi-recorder-window-probe.txt` in the temp directory
//!   (see [`crate::platform::window::PROBE_VARIABLE`]).
//! - **Not in the video.** On macOS the recorder's capture filter excludes by name every process
//!   called `komorebi-recorder` (and its own), which includes the viewer. On Windows a whole
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

/// Opens the viewer window, with each of `sessions` open in a tab, and runs it until it is closed.
///
/// This blocks the calling thread: the window's event loop runs on it. Each of `sessions` is the
/// folder of a recording. They open in the background, so a session that cannot be opened does not
/// make this fail: the window comes up and shows why. With no sessions the window shows a start
/// screen with the recent sessions that still exist ([`prefs::Prefs::existing`]) and a button to
/// choose one.
///
/// `socket` is the path of the viewer's socket ([`ipc::default_socket`]), or `None` for a window
/// of its own that no other process can reach (`view --new-window`). With a socket, if a viewer is
/// already running it is asked to open `sessions` as tabs (an empty list raises it) and this
/// returns `Ok` at once without making a window; otherwise this window listens on the socket and
/// opens what arrives as tabs. If the socket cannot be used, a message is printed and the window
/// is made without it.
///
/// The window is 1500 by 920 points (at least 900 by 600), dark, and titled only when komorebi ignores it: see the [module
/// documentation](self#staying-out-of-the-way-of-what-is-being-investigated) for why. Needs a
/// window, so there is no example; the tests in `viewer::app` drive the same [`ViewerApp`]
/// headlessly.
///
/// # Errors
///
/// Returns an error saying "the viewer window failed" if the window cannot be created or its event
/// loop fails (for example, there is no display or no usable graphics adapter).
pub fn run(sessions: Vec<PathBuf>, socket: Option<&Path>) -> eyre::Result<()> {
    let bound = match socket {
        Some(socket) => {
            let sessions: Vec<PathBuf> = sessions.iter().map(|path| ipc::absolute(path)).collect();
            let request = ipc::Request {
                open: sessions,
                focus: true,
            };
            match ipc::claim(socket, &request) {
                // a viewer is running and has taken the sessions as tabs
                Ok(ipc::Claim::Forwarded) => return Ok(()),
                Ok(ipc::Claim::Serving(bound)) => Some(bound),
                // not being able to share a window is no reason not to show the recording
                Err(error) => {
                    eprintln!("could not reach or start the viewer's socket: {error}");
                    None
                }
            }
        }
        None => None,
    };

    // only a viewer that is going to open a window needs komorebi to ignore it, and it must be
    // told before the window exists, so komorebi never sees a titled window it was not told to ignore
    let title = crate::komorebi_ignore::prepare_window().title;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // titled only when komorebi ignores it (or is not running), so opening the viewer does
            // not move the layout of the machine whose behavior is being investigated
            .with_title(title)
            .with_app_id("komorebi-recorder")
            .with_icon(crate::icon::window_icon())
            .with_title_shown(false)
            .with_titlebar_shown(false)
            .with_fullsize_content_view(true)
            .with_inner_size([1500.0, 920.0])
            .with_min_inner_size([900.0, 600.0]),
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

            let mut app = ViewerApp::new(prefs::directory());
            for dir in &sessions {
                app.open(dir);
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
    .map_err(|error| eyre::Report::msg(format!("the viewer window failed: {error}")))
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
