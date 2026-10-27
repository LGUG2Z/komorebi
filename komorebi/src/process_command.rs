//! How the daemon answers the messages on its socket: [`listen_for_commands`] and
//! [`WindowManager::process_command`].
//!
//! A client connects to `komorebi.sock` in [`struct@DATA_DIR`](crate::DATA_DIR) and writes one
//! [`SocketMessage`] as JSON per line. Each connection is read on a separate thread, and each
//! message is handled with the window manager locked. A message that asks a question is answered
//! on the same connection, and the answer ends when the daemon closes it:
//!
//! - **State**: the window manager's [`State`], as JSON.
//! - **MonitorInformation**: the id, the name and the bounds of each monitor, as JSON.
//! - **Query**: one value as text, the answer to a [`StateQuery`].
//!
//! The others answer nothing:
//!
//! - **FocusWindow** and **CycleFocusWindow** give the focus to the container on one side of the
//!   focused one, or to the next or the previous one in the workspace
//!   ([`WindowManager::focus_container_in_direction`],
//!   [`WindowManager::focus_container_in_cycle_direction`]).
//! - **MoveWindow** and **CycleMoveWindow** swap the focused container with the one on one side
//!   of it, or with the next or the previous one, and tile the workspace again
//!   ([`WindowManager::move_container_in_direction`],
//!   [`WindowManager::move_container_in_cycle_direction`]).
//! - **Promote** moves the focused container to the largest tile, **PromoteSwap** swaps it with
//!   the container there (and back), **PromoteFocus** gives that container the focus, and
//!   **PromoteWindow** promotes the container on one side of the focused one
//!   ([`WindowManager::promote_container_to_front`], [`WindowManager::promote_container_swap`],
//!   [`WindowManager::promote_focus_to_front`]).
//! - **ChangeLayout**, **CycleLayout** and **FlipLayout** give the focused workspace another
//!   layout, or mirror it, and **ToggleTiling** turns its tiling off and on
//!   ([`WindowManager::change_workspace_layout_default`], [`WindowManager::cycle_layout`],
//!   [`WindowManager::flip_layout`], [`WindowManager::toggle_tiling`]).
//! - **ScrollingLayoutColumns** sets how many columns the scrolling layout shows on the focused
//!   workspace, and **LayoutRatios** the share of the work area its first columns and rows take,
//!   and both tile the workspace again.
//! - **PreselectDirection** reserves the place on one side of the focused container for the next
//!   window ([`WindowManager::preselect_container_in_direction`]), unless a place is reserved
//!   already, and **CancelPreselect** gives the reserved place up.
//! - **MouseFollowsFocus** and **ToggleMouseFollowsFocus** set whether the pointer is moved to
//!   the window that is given the focus.
//! - **ResizeWindowEdge** and **ResizeWindowAxis** resize the focused container by the
//!   [`WindowManager::resize_delta`] that **ResizeDelta** sets ([`WindowManager::resize_window`]),
//!   and **QuickSave**, **QuickLoad**, **Save** and **Load** write the resize adjustments of the
//!   focused workspace's containers to a file and read them back.
//! - **Retile** tiles the focused workspace of every monitor again, without the resize
//!   adjustments, and **RetileWithResizeDimensions** with them.
//! - **AddSubscriberSocket** and **AddSubscriberSocketWithOptions** add a subscriber by the name
//!   of its socket in the data directory, and **RemoveSubscriberSocket** removes one.
//! - **Stop** stops the daemon ([`WindowManager::stop`]).
//!
//! Once a message has been handled, every subscriber is sent a notification of it
//! ([`notify_subscribers`]).

use crate::DATA_DIR;
use crate::Notification;
use crate::NotificationEvent;
use crate::SUBSCRIPTION_SOCKET_OPTIONS;
use crate::SUBSCRIPTION_SOCKETS;
use crate::core::Axis;
use crate::core::Layout;
use crate::core::LayoutOptions;
use crate::core::OperationDirection;
use crate::core::Rect;
use crate::core::ScrollingLayoutOptions;
use crate::core::SocketMessage;
use crate::core::StateQuery;
use crate::monitor::MonitorInformation;
use crate::notify_subscribers;
use crate::state::State;
use crate::window_manager::WindowManager;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::OptionExt;
use parking_lot::Mutex;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use version::build;

/// Starts the thread that accepts connections on the window manager's
/// [`command_listener`](WindowManager::command_listener), and reads each connection on a separate
/// thread ([`read_commands_uds`])
///
/// When accepting fails, the error is logged and the listening thread is started again.
#[tracing::instrument]
pub fn listen_for_commands(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || {
        loop {
            let wm = wm.clone();

            let _ = std::thread::spawn(move || {
                let listener = wm
                    .lock()
                    .command_listener
                    .try_clone()
                    .expect("could not clone unix listener");

                tracing::info!("listening on komorebi.sock");
                for client in listener.incoming() {
                    match client {
                        Ok(stream) => {
                            let wm_clone = wm.clone();
                            std::thread::spawn(move || {
                                match read_commands_uds(&wm_clone, stream) {
                                    Ok(()) => {}
                                    Err(error) => {
                                        tracing::error!("{error}")
                                    }
                                }
                            });
                        }
                        Err(error) => {
                            tracing::error!("failed to get unix stream {}", error);
                            break;
                        }
                    }
                }
            })
            .join();

            tracing::error!("restarting failed thread");
        }
    });
}

impl WindowManager {
    /// Handles one message, writes the answer to a question to `reply`, and sends the subscribers
    /// a notification of the message
    ///
    /// A message that fails is not notified.
    ///
    /// # Errors
    ///
    /// - A query about the focused monitor, workspace or container when there is none.
    /// - The focus cannot be moved, for one of the reasons
    ///   [`WindowManager::focus_container_in_direction`] and
    ///   [`WindowManager::focus_container_in_cycle_direction`] give.
    /// - The focused container cannot be moved, for one of the reasons
    ///   [`WindowManager::move_container_in_direction`] and
    ///   [`WindowManager::move_container_in_cycle_direction`] give.
    /// - There is no focused workspace to change the layout or the tiling of, to preselect a
    ///   place in, or to resize or promote a container of, or it cannot be tiled again.
    /// - The file of `QuickSave` or `Save` cannot be written, or the file of `QuickLoad` or
    ///   `Load` cannot be read or is not a list of resize adjustments.
    /// - The window manager fails to retile, for one of the reasons
    ///   [`WindowManager::retile_all`] gives.
    /// - The answer cannot be written to `reply`.
    /// - `Stop` fails, for the reason [`WindowManager::stop`] gives.
    /// - The notification cannot be sent, for one of the reasons [`notify_subscribers`] gives.
    #[tracing::instrument(skip(self, reply, message))]
    pub fn process_command(
        &mut self,
        message: SocketMessage,
        mut reply: impl std::io::Write,
    ) -> eyre::Result<()> {
        tracing::info!("processing command: {message}");

        #[allow(clippy::useless_asref)]
        // We don't have From implemented for &mut WindowManager
        let initial_state = State::from(self.as_ref());

        match message {
            SocketMessage::Promote => self.promote_container_to_front()?,
            SocketMessage::PromoteSwap => self.promote_container_swap()?,
            SocketMessage::PromoteFocus => self.promote_focus_to_front()?,
            SocketMessage::PromoteWindow(direction) => {
                self.focus_container_in_direction(direction)?;
                self.promote_container_to_front()?
            }
            SocketMessage::FocusWindow(direction) => {
                self.focus_container_in_direction(direction)?;
            }
            SocketMessage::PreselectDirection(direction) => {
                let focused_workspace = self.focused_workspace()?;
                let mut update = false;

                if focused_workspace.preselected_container_idx.is_some() {
                    tracing::warn!(
                        "ignoring command as this workspace already has a direction preselect set"
                    );
                } else {
                    self.preselect_container_in_direction(direction)?;
                    update = true;
                }

                if update {
                    self.focused_workspace_mut()?.update()?;
                }
            }
            SocketMessage::CancelPreselect => {
                let focused_workspace = self.focused_workspace_mut()?;
                focused_workspace.cancel_preselect();
                focused_workspace.update()?;
            }
            SocketMessage::MoveWindow(direction) => {
                self.move_container_in_direction(direction)?;
            }
            SocketMessage::CycleFocusWindow(direction) => {
                self.focus_container_in_cycle_direction(direction)?;
            }
            SocketMessage::CycleMoveWindow(direction) => {
                self.move_container_in_cycle_direction(direction)?;
            }
            SocketMessage::ResizeWindowEdge(direction, sizing) => {
                self.resize_window(direction, sizing, self.resize_delta, true)?;
            }
            SocketMessage::ResizeWindowAxis(axis, sizing) => {
                match axis {
                    Axis::Horizontal => {
                        self.resize_window(
                            OperationDirection::Left,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Right,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                    }
                    Axis::Vertical => {
                        self.resize_window(
                            OperationDirection::Up,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Down,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                    }
                    Axis::HorizontalAndVertical => {
                        self.resize_window(
                            OperationDirection::Left,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Right,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Up,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                        self.resize_window(
                            OperationDirection::Down,
                            sizing,
                            self.resize_delta,
                            false,
                        )?;
                    }
                }

                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::FlipLayout(layout_flip) => self.flip_layout(layout_flip)?,
            SocketMessage::ChangeLayout(layout) => self.change_workspace_layout_default(layout)?,
            SocketMessage::CycleLayout(direction) => self.cycle_layout(direction)?,
            SocketMessage::Retile => self.retile_all(false)?,
            SocketMessage::RetileWithResizeDimensions => self.retile_all(true)?,
            SocketMessage::ToggleTiling => {
                self.toggle_tiling()?;
            }
            SocketMessage::State => {
                let state = match serde_json::to_string_pretty(&State::from(&*self)) {
                    Ok(state) => state,
                    Err(error) => error.to_string(),
                };

                tracing::info!("replying to state");

                reply.write_all(state.as_bytes())?;

                tracing::info!("replying to state done");
            }
            SocketMessage::MonitorInformation => {
                let mut monitors = vec![];
                for monitor in self.monitors() {
                    monitors.push(MonitorInformation::from(monitor));
                }

                let monitors_state = serde_json::to_string_pretty(&monitors)
                    .unwrap_or_else(|error| error.to_string());

                reply.write_all(monitors_state.as_bytes())?;
            }
            SocketMessage::Query(query) => {
                let response = match query {
                    StateQuery::FocusedMonitorIndex => self.focused_monitor_idx().to_string(),
                    StateQuery::FocusedWorkspaceIndex => self
                        .focused_monitor()
                        .ok_or_eyre("there is no monitor")?
                        .focused_workspace_idx()
                        .to_string(),
                    StateQuery::FocusedContainerIndex => self
                        .focused_workspace()?
                        .focused_container_idx()
                        .to_string(),
                    StateQuery::FocusedWindowIndex => {
                        self.focused_container()?.focused_window_idx().to_string()
                    }
                    StateQuery::Version => build::VERSION.to_string(),
                    StateQuery::FocusedWorkspaceLayout => {
                        let focused_monitor =
                            self.focused_monitor().ok_or_eyre("there is no monitor")?;

                        focused_monitor.focused_workspace_layout().map_or_else(
                            || "None".to_string(),
                            |layout| match layout {
                                Layout::Default(default_layout) => default_layout.to_string(),
                            },
                        )
                    }
                    StateQuery::FocusedContainerKind => {
                        match self.focused_workspace()?.focused_container() {
                            None => "None".to_string(),
                            Some(container) => {
                                if container.windows().len() > 1 {
                                    "Stack".to_string()
                                } else {
                                    "Single".to_string()
                                }
                            }
                        }
                    }
                };

                reply.write_all(response.as_bytes())?;
            }
            SocketMessage::ResizeDelta(delta) => {
                self.resize_delta = delta;
            }
            SocketMessage::ScrollingLayoutColumns(count) => {
                let focused_workspace = self.focused_workspace_mut()?;

                let options = match focused_workspace.layout_options {
                    Some(mut opts) => {
                        if let Some(scrolling) = &mut opts.scrolling {
                            scrolling.columns = count.into();
                        }

                        opts
                    }
                    None => LayoutOptions {
                        scrolling: Some(ScrollingLayoutOptions {
                            columns: count.into(),
                            center_focused_column: Default::default(),
                        }),
                        grid: None,
                        column_ratios: None,
                        row_ratios: None,
                    },
                };

                focused_workspace.layout_options = Some(options);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::LayoutRatios(ref columns, ref rows) => {
                use crate::core::validate_ratios;

                let focused_workspace = self.focused_workspace_mut()?;

                let mut options = focused_workspace.layout_options.unwrap_or(LayoutOptions {
                    scrolling: None,
                    grid: None,
                    column_ratios: None,
                    row_ratios: None,
                });

                if let Some(cols) = columns {
                    options.column_ratios = Some(validate_ratios(cols));
                }

                if let Some(rws) = rows {
                    options.row_ratios = Some(validate_ratios(rws));
                }

                focused_workspace.layout_options = Some(options);
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::MouseFollowsFocus(enable) => {
                self.mouse_follows_focus = enable;
            }
            SocketMessage::ToggleMouseFollowsFocus => {
                self.mouse_follows_focus = !self.mouse_follows_focus;
            }
            SocketMessage::AddSubscriberSocket(ref socket) => {
                let mut sockets = SUBSCRIPTION_SOCKETS.lock();
                let socket_path = DATA_DIR.join(socket);
                sockets.insert(socket.clone(), socket_path);
            }
            SocketMessage::AddSubscriberSocketWithOptions(ref socket, options) => {
                let mut sockets = SUBSCRIPTION_SOCKETS.lock();
                let socket_path = DATA_DIR.join(socket);
                sockets.insert(socket.clone(), socket_path);

                let mut socket_options = SUBSCRIPTION_SOCKET_OPTIONS.lock();
                socket_options.insert(socket.clone(), options);
            }
            SocketMessage::RemoveSubscriberSocket(ref socket) => {
                let mut sockets = SUBSCRIPTION_SOCKETS.lock();
                sockets.remove(socket);
            }
            SocketMessage::Stop => {
                self.stop()?;
            }
            SocketMessage::QuickSave => {
                let workspace = self.focused_workspace()?;
                let resize = &workspace.resize_dimensions;

                let quicksave_json = std::env::temp_dir().join("komorebi.quicksave.json");

                let file = OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .create(true)
                    .open(quicksave_json)?;

                serde_json::to_writer_pretty(&file, &resize)?;
            }
            SocketMessage::QuickLoad => {
                let workspace = self.focused_workspace_mut()?;

                let quicksave_json = std::env::temp_dir().join("komorebi.quicksave.json");

                let file = File::open(&quicksave_json).wrap_err(format!(
                    "no quicksave found at {}",
                    quicksave_json.display()
                ))?;

                let resize: Vec<Option<Rect>> = serde_json::from_reader(file)?;

                workspace.resize_dimensions = resize;
                self.update_focused_workspace(false, false)?;
            }
            SocketMessage::Save(ref path) => {
                let workspace = self.focused_workspace_mut()?;
                let resize = &workspace.resize_dimensions;

                let file = OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .create(true)
                    .open(path)?;

                serde_json::to_writer_pretty(&file, &resize)?;
            }
            SocketMessage::Load(ref path) => {
                let workspace = self.focused_workspace_mut()?;

                let file =
                    File::open(path).wrap_err(format!("no file found at {}", path.display()))?;

                let resize: Vec<Option<Rect>> = serde_json::from_reader(file)?;

                workspace.resize_dimensions = resize;
                self.update_focused_workspace(false, false)?;
            }
        }

        self.update_known_window_ids();

        notify_subscribers(
            Notification {
                event: NotificationEvent::Socket(message.clone()),
                state: self.as_ref().into(),
            },
            initial_state.has_been_modified(self.as_ref()),
        )?;

        tracing::info!("processed command: {message}");

        Ok(())
    }
}

/// Reads the messages on one connection, one per line, and hands each to
/// [`WindowManager::process_command`] with the connection to answer on
///
/// A message that arrives while the window manager has been locked for a second is logged and
/// dropped.
///
/// # Errors
///
/// - The connection cannot be read, or cloned to be read and written at once.
/// - A line is not a [`SocketMessage`]: the lines after it are not read.
/// - [`WindowManager::process_command`] fails: the lines after it are not read.
pub fn read_commands_uds(
    wm: &Arc<Mutex<WindowManager>>,
    mut stream: UnixStream,
) -> eyre::Result<()> {
    let reader = BufReader::new(stream.try_clone()?);
    // TODO(raggi): while this processes more than one command, if there are
    // replies there is no clearly defined protocol for framing yet - it's
    // perhaps whole-json objects for now, but termination is signalled by
    // socket shutdown.
    for line in reader.lines() {
        let message = SocketMessage::from_str(&line?)?;

        match wm.try_lock_for(Duration::from_secs(1)) {
            None => {
                tracing::warn!(
                    "could not acquire window manager lock, not processing message: {message}"
                );
            }
            Some(mut wm) => {
                wm.process_command(message.clone(), &mut stream)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use crate::core::Rect;
    use std::io::Read;
    use std::io::Write;
    use std::net::Shutdown;
    use std::path::PathBuf;
    use uuid::Uuid;

    /// A window manager over `fake`, listening on a socket for this test alone in /tmp, which is
    /// short enough for a unix socket path on macOS. The socket is removed when the test is done.
    struct Daemon {
        fake: Arc<FakeBackend>,
        wm: Arc<Mutex<WindowManager>>,
        socket: PathBuf,
    }

    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.socket);
        }
    }

    impl Daemon {
        fn new(fake: FakeBackend) -> Self {
            let fake = Arc::new(fake);
            let socket = PathBuf::from(format!("/tmp/komorebi-test-{}.sock", Uuid::new_v4()));

            let (_sender, events) = crossbeam_channel::bounded(1);

            let wm = with_backend(fake.clone(), || {
                let mut wm = WindowManager::new(events, Some(socket.clone())).unwrap();
                wm.init().unwrap();
                wm
            });

            Self {
                fake,
                wm: Arc::new(Mutex::new(wm)),
                socket,
            }
        }

        /// Writes `lines` on a new connection, has the window manager read the connection the
        /// way the daemon does, and returns what it wrote back
        fn send(&self, lines: &str) -> eyre::Result<String> {
            let mut client = UnixStream::connect(&self.socket)?;
            client.write_all(lines.as_bytes())?;
            client.shutdown(Shutdown::Write)?;

            // the client reads on another thread, so that an answer bigger than the
            // socket's buffer does not block the window manager writing it
            let reading = std::thread::spawn(move || {
                let mut reply = String::new();
                client.read_to_string(&mut reply).map(|_| reply)
            });

            let (stream, _) = self.wm.lock().command_listener.accept()?;
            let handled = with_backend(self.fake.clone(), || read_commands_uds(&self.wm, stream));

            let reply = reading.join().expect("the client does not panic")?;
            handled.map(|()| reply)
        }

        fn ask(&self, message: SocketMessage) -> eyre::Result<String> {
            self.send(&serde_json::to_string(&message)?)
        }
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn two_windows(first: u32) -> FakeBackend {
        let fake = FakeBackend::with_one_monitor();
        fake.open_window(first, "foot", "foot", "first");
        fake.open_window(first + 1, "foot", "foot", "second");
        fake
    }

    #[test]
    fn the_state_has_every_window_of_every_workspace() -> eyre::Result<()> {
        let daemon = Daemon::new(two_windows(701));

        let state: serde_json::Value = serde_json::from_str(&daemon.ask(SocketMessage::State)?)?;

        let windows = &state["monitors"]["elements"][0]["workspaces"]["elements"][0]["containers"]
            ["elements"];
        assert_eq!(windows[0]["windows"]["elements"][0]["id"], 701);
        assert_eq!(windows[1]["windows"]["elements"][0]["id"], 702);
        assert_eq!(
            windows[1]["windows"]["elements"][0]["details"]["title"],
            "second"
        );
        assert_eq!(state["mouse_follows_focus"], true);
        Ok(())
    }

    #[test]
    fn monitor_information_names_each_monitor_and_its_bounds() -> eyre::Result<()> {
        let fake = FakeBackend::with_one_monitor();
        fake.add_monitor(
            2,
            "Virtual-1",
            rect(1920, 0, 1920, 1080),
            rect(1920, 0, 1920, 1080),
        );
        let daemon = Daemon::new(fake);

        let monitors: serde_json::Value =
            serde_json::from_str(&daemon.ask(SocketMessage::MonitorInformation)?)?;

        assert_eq!(monitors.as_array().map(Vec::len), Some(2));
        assert_eq!(monitors[1]["id"], 2);
        assert_eq!(monitors[1]["device"], "Virtual-1");
        assert_eq!(monitors[1]["size"]["left"], 1920);
        assert_eq!(monitors[0]["size"]["right"], 1920);
        Ok(())
    }

    #[test]
    fn a_query_is_answered_with_one_value() -> eyre::Result<()> {
        let daemon = Daemon::new(two_windows(711));
        let query = |query| daemon.ask(SocketMessage::Query(query));

        assert_eq!(query(StateQuery::FocusedMonitorIndex)?, "0");
        assert_eq!(query(StateQuery::FocusedWorkspaceIndex)?, "0");
        assert_eq!(query(StateQuery::FocusedContainerIndex)?, "0");
        assert_eq!(query(StateQuery::FocusedWindowIndex)?, "0");
        assert_eq!(query(StateQuery::FocusedWorkspaceLayout)?, "BSP");
        assert_eq!(query(StateQuery::FocusedContainerKind)?, "Single");
        // the version is shadow-rs's description of the build, the package version first
        let version = format!("pkg_version:{}", env!("CARGO_PKG_VERSION"));
        assert!(query(StateQuery::Version)?.contains(&version));
        Ok(())
    }

    #[test]
    fn a_query_about_a_container_on_an_empty_workspace_is_an_error() {
        let daemon = Daemon::new(FakeBackend::with_one_monitor());

        let answer = daemon.ask(SocketMessage::Query(StateQuery::FocusedWindowIndex));

        assert_eq!(
            answer.map_err(|error| error.to_string()),
            Err(String::from("there is no container"))
        );
    }

    #[test]
    fn retile_puts_a_window_moved_by_hand_back_on_its_tile() -> eyre::Result<()> {
        let daemon = Daemon::new(two_windows(721));
        daemon.ask(SocketMessage::Retile)?;
        let tile = daemon.fake.rect_of(721);

        daemon
            .fake
            .move_window_by_user(721, rect(300, 300, 400, 400));
        assert_ne!(daemon.fake.rect_of(721), tile);

        assert_eq!(daemon.ask(SocketMessage::Retile)?, "");
        assert_eq!(daemon.fake.rect_of(721), tile);
        Ok(())
    }

    #[test]
    fn the_messages_on_one_connection_are_handled_in_order() -> eyre::Result<()> {
        let daemon = Daemon::new(two_windows(731));
        daemon
            .fake
            .move_window_by_user(731, rect(300, 300, 400, 400));

        let reply = daemon.send(
            "{\"type\":\"Retile\"}\n{\"type\":\"Query\",\"content\":\"FocusedContainerKind\"}\n",
        )?;

        assert_eq!(reply, "Single");
        assert_ne!(daemon.fake.rect_of(731), Some(rect(300, 300, 400, 400)));
        Ok(())
    }

    #[test]
    fn a_line_that_is_not_a_message_is_an_error() {
        let daemon = Daemon::new(FakeBackend::with_one_monitor());

        let reply = daemon.send("{\"type\":\"Nonsense\"}\n");

        assert!(reply.is_err());
    }
}
