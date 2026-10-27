//! The KWin backend: [`KWinBackend`], made by [`connect`].
//!
//! KWin has no protocol that lets another process read and set window geometry, so komorebi runs a
//! small script inside KWin (`bridge.js`) and talks to it over D-Bus:
//!
//! ```text
//!   komorebi (this process)                              KWin
//!   -----------------------                              ----
//!   owns org.komorebi.Bridge   <--- Event(json) -------  script reports what it sees
//!                                                        (window added, focus, minimize, ...)
//!   answers Poll(json) late    ---- batch of commands -> script carries them out
//! ```
//!
//! Events are delivered in order on an interface that handles calls inline; commands go out as the
//! answer to a `Poll` call the script makes on a separate object, and that the daemon holds open
//! until it has something to send, so waiting for work never blocks an event.
//!
//! All of the decisions about what an event means are made in [`state`], which is pure and tested
//! without a compositor. This module is the plumbing around it: the D-Bus service, the held call,
//! and loading the script into KWin.
//!
//! The script is type-checked by `tsc` from its JSDoc comments and tested with `node --test`
//! against a fake KWin (`just check-js`). Its tests and the tests of this module and of [`state`]
//! read the same messages, in `fixtures/`, so that what one side sends is what the other reads.
//!
//! The backend answers every query from the [`state::Table`], turns what KWin reports into the
//! window events of [`crate::window_manager_event`], and queues every command of [`Backend`] for
//! the script, which carries it out in KWin.
//!
//! `docs/dev/PLATFORM.md` has the D-Bus names, the files and the environment variables involved,
//! and `docs/dev/DESIGN.md` the reasons for this design.

#![deny(missing_docs)]

pub mod state;

use crate::DATA_DIR;
use crate::backend::Backend;
use crate::backend::MonitorInfo;
use crate::backend::WindowInfo;
use crate::core::Rect;
use crate::window_manager_event_listener;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use parking_lot::Condvar;
use parking_lot::Mutex;
use serde_json::Value;
use serde_json::json;
use state::Output;
use state::Table;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use tokio::runtime::Handle;
use tokio::sync::Notify;
use zbus::Connection;
use zbus::interface;
use zbus::proxy::Proxy;

const SERVICE: &str = "org.komorebi.Bridge";
const PATH: &str = "/org/komorebi/Bridge";
const POLL_PATH: &str = "/org/komorebi/Bridge/Poll";
const IFACE: &str = "org.komorebi.Bridge";
const POLL_IFACE: &str = "org.komorebi.Bridge.Poll";
const SCRIPT_NAME: &str = "komorebi-bridge";
const BRIDGE_JS: &str = include_str!("bridge.js");

/// How long a `Poll` is held open when there is nothing to send. Below QtDBus's 25 second call
/// timeout, which would otherwise fail the call.
const HOLD_MS: u64 = 10_000;

/// Commands that arrive within this long of each other go out as one batch
const COALESCE: Duration = Duration::from_millis(1);

/// The state the D-Bus objects, the backend and the thread that runs the connection share
struct Shared {
    table: Mutex<Table>,
    /// commands for the script, oldest first
    queue: Mutex<VecDeque<Value>>,
    wake: Notify,
    batch: AtomicU64,
    /// set once the script has reported its first snapshot
    ready: (Mutex<bool>, Condvar),
    cursor_waiters: Mutex<HashMap<u64, std::sync::mpsc::Sender<(i32, i32)>>>,
    next_token: AtomicU64,
    handle: Handle,
    /// false once the thread that runs the runtime has ended: the runtime is then being shut down,
    /// and blocking on it panics
    alive: AtomicBool,
    connection: Mutex<Option<Connection>>,
}

impl Shared {
    /// Run something on the runtime and wait for it, unless the runtime is gone: once its thread
    /// has ended, tokio panics rather than run it
    fn block_on<F: std::future::Future>(&self, future: F) -> eyre::Result<F::Output> {
        if !self.alive.load(Ordering::SeqCst) {
            eyre::bail!("KWin is gone");
        }
        Ok(self.handle.block_on(future))
    }

    fn push(&self, command: Value) {
        self.queue.lock().push_back(command);
        self.wake.notify_one();
    }

    /// Route what the table decided to where it goes
    fn dispatch(&self, outputs: Vec<Output>) {
        for output in outputs {
            match output {
                Output::Event(event) => window_manager_event_listener::send_notification(event),
                Output::Cursor { token, x, y } => {
                    if let Some(waiter) = self.cursor_waiters.lock().remove(&token) {
                        let _ = waiter.send((x, y));
                    }
                }
            }
        }
    }

    fn mark_ready(&self) {
        *self.ready.0.lock() = true;
        self.ready.1.notify_all();
    }

    async fn connection(&self) -> eyre::Result<Connection> {
        self.connection
            .lock()
            .clone()
            .ok_or_eyre("not connected to the session bus")
    }
}

/// script -> daemon. `spawn = false` makes zbus handle these inline and in the order received (the
/// default, a task per call, was seen to reorder events); the price is that a handler that waits
/// blocks every later call on the connection, so these never wait.
struct Events {
    shared: Arc<Shared>,
}

#[interface(name = "org.komorebi.Bridge", spawn = false)]
impl Events {
    async fn event(&self, json: String) {
        let value: Value = match serde_json::from_str(&json) {
            Ok(value) => value,
            Err(error) => {
                tracing::error!("bad event from the bridge ({error}): {json}");
                return;
            }
        };

        let is_snapshot = value["type"] == "snapshot";
        let outputs = self.shared.table.lock().handle(&value, Instant::now());
        self.shared.dispatch(outputs);

        if is_snapshot {
            self.shared.mark_ready();
        }
    }
}

/// daemon -> script, on its own object so that the held call is spawned and cannot block `Events`
struct Poll {
    shared: Arc<Shared>,
}

#[interface(name = "org.komorebi.Bridge.Poll")]
impl Poll {
    /// `args` is JSON `{"hold_ms": n}`. Returns a JSON array of command batches: as soon as there
    /// are commands, or empty once `hold_ms` has passed.
    async fn poll(&self, args: String) -> String {
        let hold_ms = serde_json::from_str::<Value>(&args)
            .ok()
            .and_then(|v| v["hold_ms"].as_u64())
            .unwrap_or(HOLD_MS);

        let deadline = tokio::time::Instant::now() + Duration::from_millis(hold_ms);

        loop {
            if !self.shared.queue.lock().is_empty() {
                // let a burst finish arriving, so that a retile is one batch and not twenty
                tokio::time::sleep(COALESCE).await;
                let cmds: Vec<Value> = self.shared.queue.lock().drain(..).collect();
                let batch = self.shared.batch.fetch_add(1, Ordering::Relaxed) + 1;
                return json!([{ "batch": batch, "cmds": cmds }]).to_string();
            }

            if tokio::time::timeout_at(deadline, self.shared.wake.notified())
                .await
                .is_err()
            {
                return "[]".to_string();
            }
        }
    }
}

/// The desktop of a running KWin, read and changed through the script komorebi loads into it.
///
/// The queries are answered from a [`state::Table`] that the script's events keep up to date, so
/// they do not wait for KWin, except [`Backend::cursor_pos`]: only the script can read the pointer,
/// so it is asked, and given 250 ms to answer.
///
/// A command ([`Backend::set_rect`], [`Backend::focus`] and the rest) is queued for the script and
/// returns: the script gets the queue as the answer to its held `Poll` call, and carries the
/// commands out in order. Commands queued within a millisecond of each other go out as one batch,
/// so a retile reaches KWin at once. A command is an error when the table does not know the
/// window; a command for a window that has just closed is sent, and the script reports that it
/// could not carry it out, which is logged.
///
/// The script cannot move the pointer, so the pointer follows the focus through KWin's own "Move
/// Mouse to Focus" action, over `kglobalaccel`: it lands in the middle of the window KWin has
/// focused, whatever rectangle [`Backend::center_cursor_in`] is given. A KWin without global
/// shortcuts leaves the pointer where it is.
///
/// It is made by [`connect`].
pub struct KWinBackend {
    shared: Arc<Shared>,
}

/// Connects to the session bus, loads the script into KWin, and waits for it to report what is on
/// the desktop. Returns once that first report has arrived, so that the window manager can read
/// the desktop straight away.
///
/// The connection is served by a thread of its own (`kwin-bridge-main`), which runs until the
/// daemon exits.
///
/// # Errors
///
/// - The session bus cannot be reached (`DBUS_SESSION_BUS_ADDRESS` is not set, or names a bus
///   that is not there), or `org.komorebi.Bridge` is already owned on it: another komorebi is
///   running in the session.
/// - KWin is not on the session bus, or refuses the script.
/// - The script was loaded but did not report the desktop within 10 seconds. KWin logs a script's
///   errors to the journal (`journalctl --user -t kwin_wayland`).
/// - The script cannot be written to [`struct@DATA_DIR`].
///
/// # Example
///
/// This is how the daemon installs the backend. The VM test `vm-kwin-events`
/// (`nix/tests/vm/suites/kwin-events`) starts the daemon against a real KWin; the example needs
/// one, so it is only compiled here.
///
/// ```rust,no_run
/// use komorebi::backend;
/// use komorebi::kwin;
///
/// let kwin_backend = kwin::connect()?;
/// backend::set_backend(kwin_backend);
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
pub fn connect() -> eyre::Result<Arc<KWinBackend>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("kwin-bridge")
        .enable_all()
        .build()?;

    let shared = Arc::new(Shared {
        table: Mutex::new(Table::new()),
        queue: Mutex::new(VecDeque::new()),
        wake: Notify::new(),
        batch: AtomicU64::new(0),
        ready: (Mutex::new(false), Condvar::new()),
        cursor_waiters: Mutex::new(HashMap::new()),
        next_token: AtomicU64::new(1),
        handle: runtime.handle().clone(),
        alive: AtomicBool::new(true),
        connection: Mutex::new(None),
    });

    let (started_tx, started_rx) = std::sync::mpsc::channel::<eyre::Result<()>>();
    let thread_shared = shared.clone();

    std::thread::Builder::new()
        .name("kwin-bridge-main".into())
        .spawn(move || {
            let alive = thread_shared.clone();
            runtime.block_on(async move {
                match start(thread_shared.clone()).await {
                    Ok(()) => {
                        let _ = started_tx.send(Ok(()));
                        // the connection is served by this runtime for as long as the daemon runs
                        std::future::pending::<()>().await;
                    }
                    Err(error) => {
                        let _ = started_tx.send(Err(error));
                    }
                }
            });
            // the runtime goes when this thread does
            alive.alive.store(false, Ordering::SeqCst);
        })?;

    started_rx
        .recv()
        .map_err(|_| eyre::Report::msg("the bridge thread exited before starting"))??;

    // the script reports the desktop as soon as it runs
    {
        let (lock, condvar) = &shared.ready;
        let mut ready = lock.lock();
        if !*ready {
            condvar.wait_for(&mut ready, Duration::from_secs(10));
        }
        if !*ready {
            eyre::bail!(
                "KWin loaded the bridge script but it never reported in; \
                 check `journalctl --user -t kwin_wayland` for script errors"
            );
        }
    }

    Ok(Arc::new(KWinBackend { shared }))
}

async fn start(shared: Arc<Shared>) -> eyre::Result<()> {
    let connection = zbus::connection::Builder::session()?
        .name(SERVICE)?
        .serve_at(
            PATH,
            Events {
                shared: shared.clone(),
            },
        )?
        .serve_at(
            POLL_PATH,
            Poll {
                shared: shared.clone(),
            },
        )?
        .build()
        .await
        .map_err(|error| {
            eyre::Report::msg(format!(
                "could not connect to the session bus as {SERVICE} \
                 (is another komorebi running, and is DBUS_SESSION_BUS_ADDRESS set?): {error}"
            ))
        })?;

    *shared.connection.lock() = Some(connection.clone());
    load_script(&connection).await
}

/// Write the bridge to disk, where KWin can read it, and have KWin run it
async fn load_script(connection: &Connection) -> eyre::Result<()> {
    let cfg = json!({
        "service": SERVICE,
        "path": PATH,
        "iface": IFACE,
        "poll_path": POLL_PATH,
        "poll_iface": POLL_IFACE,
        "hold_ms": HOLD_MS,
    });

    std::fs::create_dir_all(&*DATA_DIR)?;
    let path = DATA_DIR.join("bridge.js");
    std::fs::write(&path, format!("var CFG = {cfg};\n{BRIDGE_JS}"))?;

    let scripting = Proxy::new(
        connection,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )
    .await
    .map_err(|e| eyre::Report::msg(format!("KWin is not on the session bus: {e}")))?;

    // a script from an earlier run (komorebi crashed, or was restarted) is replaced
    let _: bool = scripting.call("unloadScript", &(SCRIPT_NAME,)).await?;

    let path = path.to_string_lossy().to_string();
    let id: i32 = scripting
        .call("loadScript", &(path.as_str(), SCRIPT_NAME))
        .await?;

    let script = Proxy::new(
        connection,
        "org.kde.KWin",
        format!("/Scripting/Script{id}"),
        "org.kde.kwin.Script",
    )
    .await?;
    script.call_method("run", &()).await?;

    tracing::info!("loaded the KWin bridge script (script id {id})");
    Ok(())
}

/// KWin's own "Move Mouse to Focus" action: the scripting API cannot move the pointer
async fn move_mouse_to_focus(shared: &Shared) -> eyre::Result<()> {
    let connection = shared.connection().await?;
    let component = Proxy::new(
        &connection,
        "org.kde.kglobalaccel",
        "/component/kwin",
        "org.kde.kglobalaccel.Component",
    )
    .await?;
    component
        .call_method("invokeShortcut", &("MoveMouseToFocus",))
        .await?;
    Ok(())
}

fn command_rect(uuid: &str, rect: &Rect) -> Value {
    json!({ "op": "geo", "id": uuid, "x": rect.left, "y": rect.top, "w": rect.right, "h": rect.bottom })
}

impl KWinBackend {
    fn command(&self, id: u32, build: impl FnOnce(&str) -> Value) -> eyre::Result<()> {
        let uuid = self
            .shared
            .table
            .lock()
            .uuid(id)
            .ok_or_eyre("there is no such window")?;
        self.shared.push(build(&uuid));
        Ok(())
    }
}

impl Backend for KWinBackend {
    fn windows(&self) -> Vec<WindowInfo> {
        self.shared.table.lock().windows()
    }

    fn window(&self, id: u32) -> Option<WindowInfo> {
        self.shared.table.lock().window(id)
    }

    fn foreground_window(&self) -> Option<u32> {
        self.shared.table.lock().foreground()
    }

    fn monitors(&self) -> eyre::Result<Vec<MonitorInfo>> {
        let monitors = self.shared.table.lock().monitors();
        if monitors.is_empty() {
            eyre::bail!("KWin reported no outputs");
        }
        Ok(monitors)
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        // The script is the only thing that can read the pointer, so ask it, and fall back to where
        // it was last seen if it does not answer in time
        let token = self.shared.next_token.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = std::sync::mpsc::channel();
        self.shared.cursor_waiters.lock().insert(token, sender);
        self.shared.push(json!({ "op": "cursor", "token": token }));

        let answer = receiver.recv_timeout(Duration::from_millis(250)).ok();
        self.shared.cursor_waiters.lock().remove(&token);

        answer.or_else(|| self.shared.table.lock().cursor())
    }

    fn set_rect(&self, id: u32, rect: &Rect) -> eyre::Result<()> {
        tracing::debug!("asking for window {id} to be at {rect:?}");
        self.command(id, |uuid| command_rect(uuid, rect))?;
        self.shared.table.lock().note_requested_rect(id, *rect);
        Ok(())
    }

    fn focus(&self, id: u32, mouse_follows_focus: bool) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "activate", "id": uuid }))?;

        if mouse_follows_focus {
            // the pointer can only follow once the window really is the active one
            let shared = self.shared.clone();
            self.shared.handle.spawn(async move {
                for _ in 0..60 {
                    if shared.table.lock().foreground() == Some(id) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }

                if let Err(error) = move_mouse_to_focus(&shared).await {
                    tracing::debug!("could not move the mouse to the focused window: {error}");
                }
            });
        }

        Ok(())
    }

    fn raise(&self, id: u32) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "raise", "id": uuid }))
    }

    fn minimize(&self, id: u32) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "min", "id": uuid, "v": true }))
    }

    fn unminimize(&self, id: u32) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "min", "id": uuid, "v": false }))
    }

    fn close(&self, id: u32) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "close", "id": uuid }))
    }

    fn center_cursor_in(&self, _rect: &Rect) -> eyre::Result<()> {
        // KWin can center the pointer on the active window and nothing else; the WM calls this
        // right after focusing the window in question
        let shared = self.shared.clone();
        self.shared.handle.spawn(async move {
            if let Err(error) = move_mouse_to_focus(&shared).await {
                tracing::debug!("could not move the mouse to the focused window: {error}");
            }
        });
        Ok(())
    }

    fn shutdown(&self) {
        // the script goes with komorebi: KWin would otherwise keep running it, reporting to nobody
        let shared = self.shared.clone();

        let _ = self.shared.block_on(async move {
            if let Ok(connection) = shared.connection().await
                && let Ok(scripting) = Proxy::new(
                    &connection,
                    "org.kde.KWin",
                    "/Scripting",
                    "org.kde.kwin.Scripting",
                )
                .await
            {
                let _: Result<bool, _> = scripting.call("unloadScript", &(SCRIPT_NAME,)).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend connected to nothing: the table, the queue and the `Poll` object, on a runtime of
    /// the test's own
    fn offline() -> (tokio::runtime::Runtime, KWinBackend) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();

        let shared = Arc::new(Shared {
            table: Mutex::new(Table::new()),
            queue: Mutex::new(VecDeque::new()),
            wake: Notify::new(),
            batch: AtomicU64::new(0),
            ready: (Mutex::new(false), Condvar::new()),
            cursor_waiters: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
            handle: runtime.handle().clone(),
            alive: AtomicBool::new(true),
            connection: Mutex::new(None),
        });

        (runtime, KWinBackend { shared })
    }

    /// The answer the bridge's next `Poll` gets
    fn next_poll(runtime: &tokio::runtime::Runtime, backend: &KWinBackend) -> Value {
        let poll = Poll {
            shared: backend.shared.clone(),
        };
        let reply = runtime.block_on(poll.poll(json!({ "hold_ms": 0 }).to_string()));
        serde_json::from_str(&reply).unwrap()
    }

    /// The commands of a session as the backend sends them: `bridge.test.js` answers the bridge's
    /// `Poll` with each batch once the bridge has sent the events up to its `after_seq`
    #[test]
    fn the_commands_of_a_session_go_to_the_bridge_as_the_bridges_tests_answer_it() {
        let (runtime, backend) = offline();
        let events: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/events.json")).unwrap();
        let commands: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/commands.json")).unwrap();

        // what the window manager asks of the backend, batch by batch
        let asks: [fn(&KWinBackend); 2] = [
            |backend| {
                // nothing answers the query here, so the pointer is where the snapshot had it
                assert_eq!(backend.cursor_pos(), Some((481, 525)));
            },
            |backend| {
                let left = |left| Rect {
                    left,
                    top: 0,
                    right: 960,
                    bottom: 1050,
                };
                // the editor is 1, the terminal 2 and the dialog 3
                backend.set_rect(1, &left(0)).unwrap();
                backend.focus(1, false).unwrap();
                backend.raise(1).unwrap();
                backend.minimize(1).unwrap();
                backend.unminimize(1).unwrap();
                backend.close(3).unwrap();
                // the terminal has closed, and the table still knows it: the bridge gives it back
                backend.set_rect(2, &left(960)).unwrap();
            },
        ];
        assert_eq!(asks.len(), commands.len());

        let mut handled = 0;
        for (command, ask) in commands.iter().zip(asks) {
            // the table, as the events the bridge had sent by then left it
            let after = command["after_seq"].as_u64().unwrap() as usize;
            for event in &events[handled..after] {
                backend.shared.table.lock().handle(event, Instant::now());
            }
            handled = after;

            ask(&backend);
            assert_eq!(next_poll(&runtime, &backend), json!([command["batch"]]));
        }
    }

    #[test]
    fn a_command_about_a_window_the_table_does_not_know_is_an_error_and_sends_nothing() {
        let (runtime, backend) = offline();

        assert!(backend.focus(7, false).is_err());
        assert_eq!(next_poll(&runtime, &backend), json!([]));
    }

    #[test]
    fn a_poll_with_nothing_to_send_is_answered_with_nothing_once_its_hold_is_over() {
        let (runtime, backend) = offline();

        assert_eq!(next_poll(&runtime, &backend), json!([]));
    }
}
