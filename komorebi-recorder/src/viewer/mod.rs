//! The viewer: a window for playing and scrubbing a recording with everything that happened in it
//! laid out on a timeline.
//!
//! It is started by `komorebi-recorder view [SESSION]` ([`crate::cli::ViewArgs`]), which calls
//! [`run`], and by the "Open session..." and "Open last" buttons of the recorder window
//! ([`crate::app`]), which start that command in a process of its own so that reviewing a
//! recording cannot disturb one being made. It is an `egui` application run by `eframe`.
//!
//! ```text
//! +-------------------------------------------------+
//! | Open...  session name  counts and warnings      |  top bar
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
//! | [`keymap`] | the keys: a key press becomes an [`Action`](keymap::Action) |
//! | [`timeline`] | the arithmetic of the timeline: the visible stretch, the ruler, dense lanes |
//! | [`loaded`] | the session as the window draws it, read once |
//! | [`prefs`] | what the viewer remembers between runs |
//! | [`theme`] | colors |
//! | `app` | [`ViewerApp`], [`Screen`] and [`Open`], the state of the window and how it is kept in step |
//! | `lanes` | the transport bar and the timeline lanes, and what a mouse does on them |
//! | `inspector` | the panel on the right and its five tabs |
//! | `video` | the pictures and the state overlay drawn over them |
//! | `composer` | the form for marking a bug region, and editing, deleting and bringing one back |
//!
//! The first five are public, because they are plain values and arithmetic that need no window. The
//! rest draw, so they need an `egui::Context`: their items are documented, and there are no
//! examples for them. They are covered by the tests in `viewer::app`, which drive the whole window
//! headlessly with real key events.
//!
//! # Staying out of the way of what is being investigated
//!
//! The viewer is used on the machine whose behavior is being investigated, so it must neither be
//! tiled by komorebi nor appear in a recording being made.
//!
//! - **Not tiled.** The window has an empty title ([`run`] builds it with `with_title("")`), and
//!   komorebi leaves windows with no title alone: komorebi for Mac does not manage a window that
//!   has none. The three settings that also hide the title bar apply on macOS only. Whether
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
pub mod keymap;
mod lanes;
pub mod loaded;
pub mod prefs;
pub mod theme;
pub mod timeline;
mod video;

use color_eyre::eyre;
use eframe::egui;
use std::path::PathBuf;

pub use app::Open;
pub use app::Screen;
pub use app::ViewerApp;

/// Opens the viewer window, with `session` open if given, and runs it until it is closed.
///
/// This blocks the calling thread: the window's event loop runs on it. `session` is the folder of a
/// recording. It opens in the background, so a session that cannot be opened does not make this
/// fail: the window comes up and shows why. With no `session` the window shows a start screen with
/// the recent sessions that still exist ([`prefs::Prefs::existing`]) and a button to choose one.
///
/// The window is 1500 by 920 points (at least 900 by 600), dark, and untitled: see the [module
/// documentation](self#staying-out-of-the-way-of-what-is-being-investigated) for why. Needs a
/// window, so there is no example; the tests in `viewer::app` drive the same [`ViewerApp`]
/// headlessly.
///
/// # Errors
///
/// Returns an error saying "the viewer window failed" if the window cannot be created or its event
/// loop fails (for example, there is no display or no usable graphics adapter).
pub fn run(session: Option<PathBuf>) -> eyre::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // no title: komorebi leaves untitled windows alone, so opening the viewer does not
            // move the layout of the machine whose behavior is being investigated
            .with_title("")
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
            if let Some(dir) = &session {
                app.open(dir);
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
